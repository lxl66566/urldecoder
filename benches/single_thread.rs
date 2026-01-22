use std::{
    cmp::min,
    hint::black_box,
    io::{self, Read},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use urldecoder::decode_stream;
struct CycleReader {
    data: Vec<u8>,
    pos: usize,
}

impl CycleReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl Read for CycleReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let buf_len = buf.len();
        let data_len = self.data.len();
        let mut written = 0;

        while written < buf_len {
            let remain_in_data = data_len - self.pos;
            let remain_in_buf = buf_len - written;

            let to_copy = min(remain_in_data, remain_in_buf);
            buf[written..written + to_copy]
                .copy_from_slice(&self.data[self.pos..self.pos + to_copy]);

            written += to_copy;
            self.pos += to_copy;

            if self.pos >= data_len {
                self.pos = 0;
            }
        }
        Ok(written)
    }
}

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

    let mut group = c.benchmark_group("decode_throughput");

    group.throughput(Throughput::Bytes(STREAM_SIZE));
    group.bench_function("stream_90_text_10_url", |b| {
        b.iter(|| {
            let infinite_reader = CycleReader::new(pattern_data.clone());
            let limited_reader = infinite_reader.take(STREAM_SIZE);
            let mut sink = io::sink();
            let _ = decode_stream(
                black_box(limited_reader),
                black_box(&mut sink),
                black_box(false),
                black_box(false),
            )
            .unwrap();
        })
    });

    group.finish();
}

criterion_group!(benches, bench_decode_throughput);
criterion_main!(benches);
