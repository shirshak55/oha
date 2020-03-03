use clap::Clap;
use url::Url;

mod printer;

struct ParseDuration(std::time::Duration);

impl std::str::FromStr for ParseDuration {
    type Err = parse_duration::parse::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_duration::parse(s).map(ParseDuration)
    }
}

#[derive(Clap)]
#[clap(version = clap::crate_version!(), author = clap::crate_authors!())]
struct Opts {
    #[clap(help = "URL to request")]
    url: String,
    #[clap(help = "Number of requests", short = "n", default_value = "200")]
    n_requests: usize,
    #[clap(help = "Number of workers", short = "c", default_value = "50")]
    n_workers: usize,
    #[clap(help = "Duration", short = "z")]
    duration: Option<ParseDuration>,
}

pub struct RequestResult {
    duration: std::time::Duration,
    status: reqwest::StatusCode,
    len_bytes: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut opts: Opts = Opts::parse();
    let url = Url::parse(opts.url.as_str())?;
    let client = reqwest::Client::new();

    let start = std::time::Instant::now();
    let res = if let Some(ParseDuration(duration)) = opts.duration.take() {
        work_duration(
            || async {
                let s = std::time::Instant::now();
                let resp = client.get(url.clone()).send().await?;
                let status = resp.status();
                let len_bytes = resp.bytes().await?.len();
                let duration = std::time::Instant::now() - s;
                Ok::<_, anyhow::Error>(RequestResult {
                    duration,
                    status,
                    len_bytes,
                })
            },
            duration,
            opts.n_workers,
        )
        .await
    } else {
        let mut tasks = Vec::new();
        for _ in 0..opts.n_requests {
            tasks.push(async {
                let s = std::time::Instant::now();
                let resp = client.get(url.clone()).send().await?;
                let status = resp.status();
                let len_bytes = resp.bytes().await?.len();
                let duration = std::time::Instant::now() - s;
                Ok::<_, anyhow::Error>(RequestResult {
                    duration,
                    status,
                    len_bytes,
                })
            });
        }
        work(tasks, opts.n_workers).await
    };

    let res: Vec<_> = res.into_iter().map(|v| v.into_iter()).flatten().collect();
    let duration = std::time::Instant::now() - start;

    printer::print(res, duration);

    Ok(())
}

async fn work<T, I: IntoIterator<Item = impl std::future::Future<Output = T>>>(
    tasks: I,
    n_workers: usize,
) -> Vec<Vec<T>> {
    let injector = crossbeam::deque::Injector::new();

    for t in tasks {
        injector.push(t);
    }

    futures::future::join_all((0..n_workers).map(|_| async {
        let mut ret = Vec::new();
        while let crossbeam::deque::Steal::Success(w) = injector.steal() {
            ret.push(w.await);
        }
        ret
    }))
    .await
}

async fn work_duration<T, F: std::future::Future<Output = T>>(
    task_generator: impl Fn() -> F,
    duration: std::time::Duration,
    n_workers: usize,
) -> Vec<Vec<T>> {
    let start = std::time::Instant::now();
    futures::future::join_all((0..n_workers).map(|_| async {
        let mut ret = Vec::new();
        while (std::time::Instant::now() - start) < duration {
            ret.push(task_generator().await);
        }
        ret
    }))
    .await
}
