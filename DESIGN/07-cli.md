# 07. CLI 仕様

`src/cli.rs` の `Cli` (clap derive) と `cli::main()` が実装本体。
`main.rs` は薄い `dmm_game_bot::cli::main()` 呼び出しのみ。

## 7.1 起動シーケンス

```text
1. Cli::parse()                     // clap derive
2. init_logging(verbose)            // tracing-subscriber + JstTime
3. set_dpi_aware()                  // platform::dpi
4. Config::load_from_file(--config) // 起動時バリデーション含む
5. CLI 上書き適用 (templates_dir / window_title / post_battle_min_wait_ms)
6. dry_run_override = match (--dry-run, --live):
     (true,  false) → Some(true)
     (false, true)  → Some(false)
     (false, false) → None  // 設定ファイルの safety.dry_run に従う
7. サブコマンドにディスパッチ
```

## 7.2 グローバルフラグ

```
dmm-game-bot.exe [OPTIONS] [SUBCOMMAND]
```

| フラグ | 型 | 既定 | 説明 |
|---|---|---|---|
| `-c`, `--config <PATH>` | `PathBuf` | `config/default.toml` | 設定ファイルパス |
| `--dry-run` | flag | (排他: `--live`) | クリック送信を抑止。`safety.dry_run` を強制 true |
| `--live` | flag | (排他: `--dry-run`) | 実クリックを送信。`safety.dry_run` を強制 false |
| `--templates-dir <PATH>` | `Option<PathBuf>` | - | `templates_dir` を上書き |
| `--window-title <STR>` | `Option<String>` | - | `window.title_pattern` を上書き |
| `--max-cycles <N>` | `Option<u32>` | - | `loop.max_cycles` を上書き (0=無限) |
| `--post-battle-min-wait-ms <N>` | `Option<u64>` | - | ToubatsuStart 後のハード sleep を上書き (動作確認用) |
| `-v`, `--verbose` | count | 0 | -v=DEBUG, -vv=TRACE。0=INFO |

`--dry-run` と `--live` は clap の `conflicts_with` で排他化。

## 7.3 サブコマンド

```rust
pub enum Command {
    Run,
    DetectOnce,
    Capture { output: PathBuf },     // -o, --output、既定 capture.png
}
```

省略時は `Run` がデフォルト (`cli.command.unwrap_or(Command::Run)`)。

### `run` (省略可)

通常実行。`BotEngine::new` でセットアップ後、`run_loop(cli.max_cycles)` を呼ぶ。

```bash
# ドライラン (安全側)
dmm-game-bot.exe run

# 実クリック 1 周
dmm-game-bot.exe --live --max-cycles 1 run

# 動作確認用に戦闘待機を 30 秒に短縮して 1 周
dmm-game-bot.exe -v --live --max-cycles 1 --post-battle-min-wait-ms 30000 run
```

### `detect-once`

1 フレームキャプチャして全テンプレを ROI 限定マッチし、結果を表で stdout に出力。
内部的には `engine.detect_once()` が `Vec<DetectionRow>` を返す
(`DetectionRow { template, matched, score, center }`)。

```bash
dmm-game-bot.exe -v detect-once
```

出力例:
```
TEMPLATE                     MATCH       BEST  CENTER (client)
ap_plus_button               yes       0.9923  (123, 56)
ap_recovered_use_max         no        0.4521  -
...
```

ROI / threshold 調整時の確認用。クリック発行は行わない。

### `capture --output PATH`

1 フレームキャプチャして PNG 保存。テンプレ切り出し元の素材作成用。

```bash
dmm-game-bot.exe capture -o screenshot.png
# saved 1277x693 screenshot to screenshot.png
```

## 7.4 ロギング (`init_logging` + `JstTime`)

- フォーマッタ: `tracing_subscriber::fmt`
- 環境変数 `RUST_LOG` (例: `dmm_game_bot=trace`) があればそれを優先。なければ
  `dmm_game_bot=<level>` (level は verbose カウントから決定)。
- タイムスタンプ: **JST 固定**。`JstTime::format_time` が `chrono::Utc::now()` を
  `jst_offset()` (+09:00) に変換し、`%Y-%m-%dT%H:%M:%S%.6f%:z` で出力。
  システムロケール非依存。
- `with_target(true)`: `dmm_game_bot::bot::sequence` などモジュールパスがログに出る。

verbose とログレベルの対応:

| `-v` | level |
|---|---|
| (なし) | INFO |
| `-v` | DEBUG |
| `-vv` 以上 | TRACE |

`RUST_LOG` 指定があればそれが最優先で、`-v` の効果は無視される。

## 7.5 緊急停止

`Ctrl+C` (SIGINT) で OS 標準の停止。
進行中のクリック発行は SendInput が同期で完了するまで待つ
(押下時間 60〜120ms 程度なので実害なし)。
グローバルホットキー (F12 等) は v1.1 設計書に提案があったが **採用しない方針**
(ホットキー監視のためだけに tokio を入れる重さに見合わない、と
[`02-architecture.md`](02-architecture.md) §2.3 の依存ポリシーで判断済み)。
現状は `Ctrl+C` 一択。
