use std::{
    hint::black_box,
    io::{self},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use urldecoder::decode_slice_to_writer;
#[cfg(feature = "verbose-log")]
use urldecoder::log::NoOpLogger;

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

fn bench_decode_throughput(c: &mut Criterion) {
    let pattern_data = generate_mixed_data();

    const STREAM_SIZE: u64 = 128 * 1024 * 1024;

    let mut full_data = Vec::with_capacity(STREAM_SIZE as usize);
    while full_data.len() < STREAM_SIZE as usize {
        let remain = (STREAM_SIZE as usize) - full_data.len();
        let to_copy = remain.min(pattern_data.len());
        full_data.extend_from_slice(&pattern_data[..to_copy]);
    }

    let mut group = c.benchmark_group("decode_throughput");
    group.throughput(Throughput::Bytes(STREAM_SIZE));

    group.bench_function("slice_90_text_10_url", |b| {
        b.iter(|| {
            let mut sink = io::sink();
            #[cfg(feature = "verbose-log")]
            {
                let mut logger = NoOpLogger;
                decode_slice_to_writer(
                    black_box(&full_data),
                    black_box(&mut sink),
                    black_box(true),
                    black_box(&mut logger),
                )
                .unwrap()
            }
            #[cfg(not(feature = "verbose-log"))]
            {
                decode_slice_to_writer(black_box(&full_data), black_box(&mut sink), black_box(true))
                    .unwrap()
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_decode_throughput);
criterion_main!(benches);
