use base64::Engine;
use bench_rs::{bench, Bencher, Stats};
use rand::Rng;
use std::time::Duration;

#[test]
fn test_bencher() {
    let data = rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(1000)
        .collect::<Vec<u8>>();

    let mut bencher = Bencher::new(
        "test_bencher",
        1000,
        data.len(),
        false,
        bench_rs::GLOBAL_ALLOC,
    );
    bencher.iter(|| {
        let _ = rcnb_rs::encode(&data);
    });

    // Custom formatting
    bencher.format_fn =
        |stats: &Stats, b: &Bencher| println!("{}: custom formatting: {:?}\n", &b.name, stats);

    bencher.finish();
}

#[bench(count = 100, bytes)]
fn bench_async_with_tokio(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fut = b.async_iter(|| async {
        futures_timer::Delay::new(Duration::from_nanos(20_000_000)).await;
    });
    rt.block_on(fut);
}

#[bench(count = 100, bytes)]
fn bench_async_with_smol(b: &mut Bencher) {
    let fut = b.async_iter(|| async {
        futures_timer::Delay::new(Duration::from_nanos(20_000_000)).await;
    });
    smol::block_on(fut);
}

#[bench(count = 100, bytes)]
fn bench_async_with_async_std(b: &mut Bencher) {
    let fut = b.async_iter(|| async {
        futures_timer::Delay::new(Duration::from_nanos(20_000_000)).await;
    });
    async_std::task::block_on(fut);
}

#[bench(count = 100, bytes)]
fn bench_async_with_futures(b: &mut Bencher) {
    let fut = b.async_iter(|| async {
        futures_timer::Delay::new(Duration::from_nanos(20_000_000)).await;
    });
    futures::executor::block_on(fut);
}

#[bench(name = "test_rcnb_encoding", bytes)]
fn bench_rcnb(b: &mut Bencher) {
    let data = rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(1000)
        .collect::<Vec<u8>>();
    b.iter(|| {
        let _ = rcnb_rs::encode(&data);
    });
    b.bytes = data.len()
}

#[bench(bytes)]
fn bench_base64(b: &mut Bencher) {
    let data = rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(1000)
        .collect::<Vec<u8>>();
    b.iter(|| {
        let _ = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&data);
    });
    b.bytes = data.len()
}

#[bench(no_test)]
fn bench_no_run(_: &mut Bencher) {
    println!("no #[test]");
}
