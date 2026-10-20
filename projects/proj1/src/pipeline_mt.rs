use std::env;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::process;
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::Instant;

mod hash;
use hash::sum_block_hash;
use hash::stretch_md5;

// how many read blocks may wait in the channel for the compute thread
const CHANNEL_SIZE: usize = 4;

fn main() {

    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        println!("Usage: cargo run --release --bin pipeline_mt filepath block_size k");
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

    let k_res = args[3].parse::<u64>();
    if k_res.is_err() {
        eprintln!("Invalid k: {}", args[3]);
        return;
    }
    let k = k_res.unwrap();
    if k == 0 {
        eprintln!("k must be positive");
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

    let (tx, rx) = sync_channel::<Vec<u8>>(CHANNEL_SIZE);
    // reader thread: owns the file and the only sender
    thread::spawn(move || {
        for block_idx in 0..num_blocks
        {
            let offset = block_idx * block_size;
            let bytes_to_read = std::cmp::min(block_size, total_size - offset) as usize;

            // read
            let mut buffer = vec![0u8; bytes_to_read];
            let err = file.read_exact_at(&mut buffer, offset);
            if err.is_err() {
                eprintln!("Error reading file: {}", err.unwrap_err());
                process::exit(1);
            }
            tx.send(buffer).unwrap();
        }
    });

    let mut result = vec![0u8; 16];
    for buffer in rx {
        // compute
        let block_hash = stretch_md5(&buffer, k);
        sum_block_hash(&mut result, &block_hash);
    }
    eprintln!("time_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    print!("{} 0x", args[1].clone());
    for byte in &result {
        print!("{:02x}", byte);
    }
    println!("");

}
