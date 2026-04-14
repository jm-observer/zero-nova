use custom_utils::logger;
use log::LevelFilter::Info;
use zero_nova::run;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = logger::logger_feature("app", "debug", Info, false).build();
    custom_utils::daemon::daemon().await?;
    // remember to print msg via stdio
    run().await
}
