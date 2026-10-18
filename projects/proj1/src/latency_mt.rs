use std::env;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::process;
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::Instant;
use rand::{Rng, RngExt, SeedableRng};
use rand::rngs::StdRng;

mod hash;
use hash::sum_block_hash;

pub struct RandomOrderIterator {
    elements: Vec<u64>,
    rng: Box<dyn Rng>
}

impl RandomOrderIterator {
    pub fn with_seed(size: u64, seed: u64) -> Self {
        Self { elements: (0 as u64..size).collect() , rng: Box::new(StdRng::seed_from_u64(seed)), }
    }
}

impl Iterator for RandomOrderIterator {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.elements.is_empty() {
            None
        } else {
            let rand_idx = self.rng.random_range(0..self.elements.len());
            Some(self.elements.swap_remove(rand_idx))
        }
    }
}

const RAND_SEED: u64 = 67;
// how many bytes of read blocks may wait in the channel for the compute thread
const CHANNEL_BYTES: u64 = 64 * 1024 * 1024;

fn main() {

    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        println!("Usage: cargo run --release --bin latency_mt filepath block_size threads");
        return;
    }

    let block_size_res = args[2].parse::<u64>();
    if block_size_res.is_err() {
        eprintln!("Invalid block size: {}", args[2]);
        return;
    }
    let block_size = block_size_res.unwrap();
    if block_size == 0 {
        eprintln!("Block size must be positive");
        return;
    }

    let threads_res = args[3].parse::<usize>();
    if threads_res.is_err() {
        eprintln!("Invalid number of threads: {}", args[3]);
        return;
    }
    let threads = threads_res.unwrap();
    if threads == 0 {
        eprintln!("Number of threads must be positive");
        return;
    }

    let start = Instant::now();

    let file_res = File::open(args[1].clone());
    if file_res.is_err() {
        eprintln!("File not found: {}", args[1]);
        return;
    }
    let file = file_res.unwrap();

    let metadata = file.metadata();
    if metadata.is_err() {
        eprintln!("Error opening file: {}", args[1]);
        return;
    }
    let total_size = metadata.unwrap().len();
    let num_blocks = (total_size + block_size - 1) / block_size;
    let random_iter = RandomOrderIterator::with_seed(num_blocks, RAND_SEED);

    // split the random order between threads: i-th block goes to thread i % threads
    let mut parts: Vec<Vec<u64>> = Vec::new();
    for _ in 0..threads {
        parts.push(Vec::new());
    }
    let mut i = 0;
    for block_idx in random_iter {
        parts[i % threads].push(block_idx);
        i += 1;
    }

    let channel_size = std::cmp::max(threads as u64, CHANNEL_BYTES / block_size) as usize;
    let (tx, rx) = sync_channel::<Vec<u8>>(channel_size);
    for part in parts {
        let thread_tx = tx.clone();
        let thread_file = file.try_clone().unwrap();
        thread::spawn(move || {
            for block_idx in part {
                let offset = block_idx * block_size;
                let bytes_to_read = std::cmp::min(block_size, total_size - offset) as usize;

                // read
                let mut buffer = vec![0u8; bytes_to_read];
                let err = thread_file.read_exact_at(&mut buffer, offset);
                if err.is_err() {
                    eprintln!("Error reading file: {}", err.unwrap_err());
                    process::exit(1);
                }
                thread_tx.send(buffer).unwrap();
            }
        });
    }
    // only reader threads may hold senders, otherwise the loop below never ends
    drop(tx);

    let mut result = vec![0u8; 16];
    for buffer in rx {
        // compute
        let block_hash = md5::compute(&buffer);
        sum_block_hash(&mut result, &block_hash);
    }
    eprintln!("time_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    print!("{} 0x", args[1].clone());
    for byte in &result {
        print!("{:02x}", byte);
    }
    println!("");

}
