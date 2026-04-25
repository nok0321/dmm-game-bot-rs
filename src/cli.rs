use std::fmt;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::EnvFilter;

use crate::bot::cycle::jst_offset;
use crate::bot::BotEngine;
use crate::config::Config;
use crate::platform::dpi::set_dpi_aware;

#[derive(Debug, Parser)]
#[command(name = "dmm-game-bot", version, about = "DMM browser game automation bot")]
pub struct Cli {
    /// 設定ファイルパス。
    #[arg(short, long, default_value = "config/default.toml")]
    pub config: PathBuf,

    /// クリック送信を行わず、検出ログのみ出す。設定ファイルの safety.dry_run を強制 true 上書き。
    #[arg(long, conflicts_with = "live")]
    pub dry_run: bool,

    /// 実クリックを送信する。設定ファイルの safety.dry_run を強制 false 上書き。
    /// dry_run と排他。
    #[arg(long, conflicts_with = "dry_run")]
    pub live: bool,

    /// 設定ファイル相対のテンプレートディレクトリを上書き。
    #[arg(long)]
    pub templates_dir: Option<PathBuf>,

    /// ウィンドウタイトルパターンを上書き。
    #[arg(long)]
    pub window_title: Option<String>,

    /// 実行サイクル数の上限 (0 = 無限、設定ファイルを上書き)。
    #[arg(long)]
    pub max_cycles: Option<u32>,

    /// ToubatsuStart クリック後のハード待機 (ミリ秒) を上書き。動作確認用。
    /// 例: `--post-battle-min-wait-ms 30000` で 30 秒に短縮。
    /// 設定ファイルの loop.poll.post_battle_min_wait_ms を強制上書き。
    #[arg(long)]
    pub post_battle_min_wait_ms: Option<u64>,

    /// 詳細ログ。-v=DEBUG, -vv=TRACE。
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 通常実行 (デフォルト)。
    Run,
    /// 1 回だけ画面を検出してテンプレマッチ結果を表示する。
    DetectOnce,
    /// スクリーンショットを保存して終了する。
    Capture {
        /// 出力先 PNG パス。
        #[arg(short, long, default_value = "capture.png")]
        output: PathBuf,
    },
}

pub fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    set_dpi_aware();

    let mut config = Config::load_from_file(&cli.config)
        .with_context(|| format!("loading config {:?}", cli.config))?;

    if let Some(p) = &cli.templates_dir {
        config.templates_dir = p.clone();
    }
    if let Some(t) = &cli.window_title {
        config.window.title_pattern = t.clone();
    }
    if let Some(ms) = cli.post_battle_min_wait_ms {
        tracing::info!(
            "CLI override: loop.poll.post_battle_min_wait_ms {} -> {} ms (testing)",
            config.loop_.poll.post_battle_min_wait_ms,
            ms
        );
        config.loop_.poll.post_battle_min_wait_ms = ms;
    }

    let cmd = cli.command.unwrap_or(Command::Run);
    let dry_run_override = if cli.dry_run {
        Some(true)
    } else if cli.live {
        Some(false)
    } else {
        None
    };

    match cmd {
        Command::Run => {
            let engine = BotEngine::new(config, dry_run_override)?;
            engine.run_loop(cli.max_cycles)?;
        }
        Command::DetectOnce => {
            let engine = BotEngine::new(config, dry_run_override)?;
            let rows = engine.detect_once()?;
            println!(
                "{:<28} {:<7} {:>8}  CENTER (client)",
                "TEMPLATE", "MATCH", "BEST"
            );
            for r in rows {
                println!(
                    "{:<28} {:<7} {:>8.4}  {}",
                    r.template,
                    if r.matched { "yes" } else { "no" },
                    r.score,
                    r.center
                        .map(|(x, y)| format!("({}, {})", x, y))
                        .unwrap_or_else(|| "-".into())
                );
            }
        }
        Command::Capture { output } => {
            let engine = BotEngine::new(config, dry_run_override)?;
            let img = engine.capture_rgba()?;
            img.save(&output)
                .with_context(|| format!("saving {:?}", output))?;
            println!(
                "saved {}x{} screenshot to {}",
                img.width(),
                img.height(),
                output.display()
            );
        }
    }

    Ok(())
}

/// tracing-subscriber 用の JST タイムスタンプフォーマッタ。
/// システムロケールに依存せず必ず +09:00 で出力する。
struct JstTime;

impl FormatTime for JstTime {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        let now = chrono::Utc::now().with_timezone(&jst_offset());
        write!(w, "{}", now.format("%Y-%m-%dT%H:%M:%S%.6f%:z"))
    }
}

fn init_logging(verbose: u8) {
    let default = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("dmm_game_bot={}", default)));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_timer(JstTime)
        .try_init();
}
