#[cfg(feature = "verbose-log")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{hint::black_box, io::Write};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use tempfile::NamedTempFile;
use urldecoder::decode_file;

const SMALL_FILE_SIZE: u64 = 32 * 1024; // 32 KB
const LARGE_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

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

fn generate_data(size: u64) -> Vec<u8> {
    let pattern_data = generate_mixed_data();
    let mut full_data = Vec::with_capacity(size as usize);
    while full_data.len() < size as usize {
        let remain = (size as usize) - full_data.len();
        let to_copy = remain.min(pattern_data.len());
        full_data.extend_from_slice(&pattern_data[..to_copy]);
    }
    full_data
}

fn bench_file_decode_dry_run(c: &mut Criterion) {
    let small_data = generate_data(SMALL_FILE_SIZE);
    let large_data = generate_data(LARGE_FILE_SIZE);

    let [small_path, large_path] = [small_data, large_data].map(|x| {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(&x).unwrap();
        temp.into_temp_path()
    });

    #[cfg(feature = "verbose-log")]
    let p_counter = AtomicUsize::new(0);
    #[cfg(feature = "verbose-log")]
    let c_counter = AtomicUsize::new(0);

    let safe_suffix = if cfg!(feature = "safe") {
        " (safe)"
    } else {
        " (unsafe)"
    };

    // unsafe: 3.6112 GiB/s
    let mut small_group = c.benchmark_group("decode_file_small_dry_run");
    small_group.throughput(Throughput::Bytes(SMALL_FILE_SIZE));
    small_group.bench_function("decode_small_dry_run".to_string() + safe_suffix, |b| {
        b.iter(|| {
            #[cfg(feature = "verbose-log")]
            {
                decode_file(
                    black_box(&small_path),
                    black_box(true),
                    black_box(true),
                    black_box(false),
                    black_box(&p_counter),
                    black_box(&c_counter),
                )
            }

            #[cfg(not(feature = "verbose-log"))]
            {
                decode_file(black_box(&small_path), black_box(true), black_box(true))
            }
        })
    });
    small_group.finish();

    // unsafe: 6.6144 GiB/s
    let mut large_group = c.benchmark_group("decode_file_large_dry_run");
    large_group.throughput(Throughput::Bytes(LARGE_FILE_SIZE));
    large_group.bench_function("decode_large_dry_run".to_string() + safe_suffix, |b| {
        b.iter(|| {
            #[cfg(feature = "verbose-log")]
            {
                decode_file(
                    black_box(&large_path),
                    black_box(true),
                    black_box(true),
                    black_box(false),
                    black_box(&p_counter),
                    black_box(&c_counter),
                )
            }

            #[cfg(not(feature = "verbose-log"))]
            {
                decode_file(black_box(&large_path), black_box(true), black_box(true))
            }
        })
    });
    large_group.finish();
}

fn bench_file_decode(c: &mut Criterion) {
    let small_data = generate_data(SMALL_FILE_SIZE);
    let large_data = generate_data(LARGE_FILE_SIZE);

    #[cfg(feature = "verbose-log")]
    let p_counter = AtomicUsize::new(0);
    #[cfg(feature = "verbose-log")]
    let c_counter = AtomicUsize::new(0);

    let safe_suffix = if cfg!(feature = "safe") {
        " (safe)"
    } else {
        " (unsafe)"
    };

    // unsafe: 1.4933 GiB/s
    // safe: 1.1948 GiB/s
    let mut small_group = c.benchmark_group("decode_file_small");
    small_group.throughput(Throughput::Bytes(SMALL_FILE_SIZE));
    small_group.bench_function("decode_small".to_string() + safe_suffix, |b| {
        b.iter_batched_ref(
            || {
                let mut temp = NamedTempFile::new().unwrap();
                temp.write_all(&small_data).unwrap();
                temp.into_temp_path()
            },
            |small_path| {
                #[cfg(feature = "verbose-log")]
                {
                    decode_file(
                        black_box(&small_path),
                        black_box(true),
                        black_box(false),
                        black_box(false),
                        black_box(&p_counter),
                        black_box(&c_counter),
                    )
                }

                #[cfg(not(feature = "verbose-log"))]
                {
                    decode_file(black_box(&small_path), black_box(true), black_box(false))
                }
            },
            BatchSize::SmallInput,
        )
    });
    small_group.finish();

    // unsafe: 5.7140 GiB/s
    // safe: 2.1883 GiB/s
    let mut large_group = c.benchmark_group("decode_file_large");
    large_group.throughput(Throughput::Bytes(LARGE_FILE_SIZE));
    large_group.bench_function("decode_large".to_string() + safe_suffix, |b| {
        b.iter_batched_ref(
            || {
                let mut temp = NamedTempFile::new().unwrap();
                temp.write_all(&large_data).unwrap();
                temp.into_temp_path()
            },
            |large_path| {
                #[cfg(feature = "verbose-log")]
                {
                    decode_file(
                        black_box(&large_path),
                        black_box(true),
                        black_box(false),
                        black_box(false),
                        black_box(&p_counter),
                        black_box(&c_counter),
                    )
                }

                #[cfg(not(feature = "verbose-log"))]
                {
                    decode_file(black_box(&large_path), black_box(true), black_box(false))
                }
            },
            BatchSize::SmallInput,
        );
    });
    large_group.finish();
}

criterion_group!(benches, bench_file_decode_dry_run, bench_file_decode);
criterion_main!(benches);
