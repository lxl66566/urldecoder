use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::atomic::AtomicUsize,
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rayon::iter::{IntoParallelIterator as _, IntoParallelRefIterator, ParallelIterator};
use tempfile::TempDir;
use urldecoder::decode_file;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn generate_mixed_data() -> Vec<u8> {
    let url = "https://2.com/1?q=%E5%A4%A9%E6%B0%94";
    let url_len = url.len();

    // Total = URL / 0.1
    let total_len = url_len * 10;
    let text_len = total_len - url_len;

    let mut pattern = String::with_capacity(total_len);
    let raw_text = "This is a standard chunk of text used to simulate the payload which acts as the ninety percent of the content stream. It contains spaces and normal sentences. ";

    while pattern.len() < text_len {
        let needed = text_len - pattern.len();
        if needed >= raw_text.len() {
            pattern.push_str(raw_text);
        } else {
            pattern.push_str(&raw_text[..needed]);
        }
    }
    pattern.push_str(url);
    pattern.into_bytes()
}

fn prepare_test_env() -> (TempDir, Vec<PathBuf>, u64) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file_count = 32;
    let min_file_size = 10 * 1024 * 1024; // 10MB
    // let min_file_size = 900 * 1024; // 900KB

    let pattern = generate_mixed_data();

    let mut file_content = Vec::with_capacity(min_file_size + pattern.len());
    while file_content.len() < min_file_size {
        file_content.extend_from_slice(&pattern);
    }
    let single_file_size = file_content.len() as u64;

    let paths: Vec<PathBuf> = (0..file_count)
        .into_par_iter()
        .map(|i| {
            let file_path = dir.path().join(format!("test_file_{}.txt", i));
            let mut f = BufWriter::new(File::create(&file_path).unwrap());
            f.write_all(&file_content).unwrap();
            f.flush().unwrap();
            file_path
        })
        .collect();

    let total_bytes = single_file_size * file_count as u64;
    (dir, paths, total_bytes)
}

fn bench_decode_throughput(c: &mut Criterion) {
    let (temp_dir, paths, total_bytes) = prepare_test_env();

    let mut group = c.benchmark_group("decode_throughput");

    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_function("rayon_decode_dry_run", |b| {
        b.iter(|| {
            let processed_count = AtomicUsize::new(0);
            let changed_count = AtomicUsize::new(0);
            let escape_space = false;
            let verbose = false;
            let dry_run = true;

            paths.par_iter().for_each(|path| {
                decode_file(
                    path,
                    escape_space,
                    verbose,
                    dry_run,
                    &processed_count,
                    &changed_count,
                )
                .unwrap();
            });
        })
    });

    group.finish();
    drop(temp_dir);
}

criterion_group!(benches, bench_decode_throughput);
criterion_main!(benches);
