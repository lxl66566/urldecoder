use std::{hint::black_box, io};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use urldecoder::decode_slice_to_writer;
#[cfg(feature = "verbose-log")]
use urldecoder::log::NoOpLogger;

const STREAM_SIZE: u64 = 128 * 1024 * 1024;

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

fn generate_full_data() -> Vec<u8> {
    let pattern_data = generate_mixed_data();

    let mut full_data = Vec::with_capacity(STREAM_SIZE as usize);
    while full_data.len() < STREAM_SIZE as usize {
        let remain = (STREAM_SIZE as usize) - full_data.len();
        let to_copy = remain.min(pattern_data.len());
        full_data.extend_from_slice(&pattern_data[..to_copy]);
    }
    full_data
}

fn bench_decode(c: &mut Criterion) {
    let full_data = generate_full_data();
    let mut group = c.benchmark_group("decode_slice");
    group.throughput(Throughput::Bytes(STREAM_SIZE));

    // 9.4580 GiB/s
    group.bench_function("slice_to_sink", |b| {
        b.iter(|| {
            let mut sink = io::sink();

            #[cfg(feature = "verbose-log")]
            {
                let mut logger = NoOpLogger;
                decode_slice_to_writer(
                    black_box(&full_data),
                    black_box(&mut sink),
                    black_box(true),
                    #[cfg(feature = "verbose-log")]
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
    // 7.8869 GiB/s
    group.bench_function("decode_in_place", |b| {
        b.iter_batched_ref(
            || full_data.clone(),
            |full_data| {
                #[cfg(feature = "verbose-log")]
                {
                    let mut logger = VerboseLogger::new();
                    decode_in_place(black_box(&mut full_data), black_box(true), &mut logger)
                }
                #[cfg(not(feature = "verbose-log"))]
                {
                    use urldecoder::decode_in_place;

                    decode_in_place(black_box(full_data), black_box(true))
                }
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_decode);
criterion_main!(benches);
