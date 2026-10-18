use std::env;
use std::fs::File;
use std::time::Instant;
use memmap2::Mmap;

mod hash;
use hash::sum_block_hash;

fn main() {

    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        println!("Usage: cargo run --release --bin throughput_mmap filepath block_size");
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

    // the file must not be modified while it is mapped
    let mmap_res = unsafe { Mmap::map(&file) };
    if mmap_res.is_err() {
        eprintln!("Error mapping file: {}", mmap_res.unwrap_err());
        return;
    }
    let mmap = mmap_res.unwrap();

    let mut result = vec![0u8; 16];
    for block_idx in 0..num_blocks
    {
        let offset = block_idx * block_size;
        let bytes_to_read = std::cmp::min(block_size, total_size - offset);

        // read: no copy, the block is a slice of the mapped file
        let block = &mmap[offset as usize..(offset + bytes_to_read) as usize];

        // compute
        let block_hash = md5::compute(block);
        sum_block_hash(&mut result, &block_hash);
    }
    eprintln!("time_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    print!("{} 0x", args[1].clone());
    for byte in &result {
        print!("{:02x}", byte);
    }
    println!("");

}
