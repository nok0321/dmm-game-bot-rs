# 10. ビルドと配布

## 10.1 ビルドコマンド

```bash
# 開発ビルド
cargo build

# リリースビルド (単一 exe)
cargo build --release --target x86_64-pc-windows-msvc
```

検証 Rust: 1.94.0 (msvc)、検証 OS: Windows 11 Home 10.0.26200。

成果物: `target/release/dmm-game-bot.exe` (≒ 6.2 MB、2026-04-25 ベータ時点)。

## 10.2 リリースプロファイル (`Cargo.toml`)

```toml
[profile.release]
lto = true            # Link-Time Optimization (実行速度 + サイズ削減)
codegen-units = 1     # 単一コード生成ユニット (LTO の効果を最大化)
strip = true          # シンボル削除でサイズを更に削減
opt-level = 3         # 最大最適化
panic = "abort"       # アンワインドテーブル削除でサイズ削減 (panic は即終了)
```

`panic = "abort"` の意味:
- panic 発生時にスタックアンワインドせず即プロセス終了。
- バイナリサイズが 100KB〜数 MB 単位で縮む。
- 副作用: panic を catch するライブラリ (Rust の `catch_unwind` 等) は機能しない。
  本ツールは catch する箇所がないので問題なし。

## 10.3 静的リンク (`.cargo/config.toml`)

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

これにより:
- C ランタイム (vcruntime, msvcp) を **静的リンク**。
- **Visual C++ 再頒布パッケージ不要** のスタンドアロン exe になる。
- 配布先 PC に追加インストールが要らない (Windows 10/11 デフォルト環境で動く)。

## 10.4 リリース構成

```
release/
├── dmm-game-bot.exe               # 単一バイナリ (≒ 6.2 MB)
├── config/
│   └── default.toml               # 既定設定 (運用前に title_pattern を調整)
└── templates/                      # 9 種テンプレ画像
    ├── ap_plus_button.png
    ├── ap_recovered_use_max.png
    ├── reisseki_zero_guard.png    # ガード必須テンプレ
    ├── use_button.png
    ├── tap_indicator.png
    ├── toubatsu_button.png
    ├── toubatsu_start.png
    ├── next_button.png
    └── close_button.png
```

`templates/` は exe と分離して同梱することで、ゲーム側 UI 変更時にテンプレート
差し替えのみで対応可能にする。

## 10.5 依存クレートのコンパイル

`Cargo.lock` をリポジトリに含めている (バイナリリポジトリ規約に従う)。
これにより、別環境でビルドしても同じバージョンが解決される。

主要依存クレートの選定理由は [`02-architecture.md`](02-architecture.md) を参照。
**OpenCV / xcap / GPU template-matching クレートは採用しない**:
- OpenCV: DLL 依存 → 単一 exe 配布が困難
- xcap: フォールバック用に検討したが、PrintWindow + BitBlt で十分性能が出ているため不採用
- GPU クレート (WGPU): CPU で 1 周あたり < 5% に収まっており、初期は不要

## 10.6 動作確認手順

ビルド直後の確認シーケンス (`CHECKPOINT.md` から転記):

```bash
# テンプレロードと ROI が機能していることの確認
target/release/dmm-game-bot.exe -v detect-once

# ドライラン 1 周 (クリック発行なし)
target/release/dmm-game-bot.exe --max-cycles 1 run

# 実クリック 1 周
target/release/dmm-game-bot.exe -v --live --max-cycles 1 run

# 動作確認用に戦闘待機を 30 秒に短縮して 1 周
target/release/dmm-game-bot.exe -v --live --max-cycles 1 --post-battle-min-wait-ms 30000 run

# 実クリック 2 周 (周回継続成立確認)
target/release/dmm-game-bot.exe -v --live --max-cycles 2 run
```

## 10.7 既知のビルド時注意点

| 項目 | 内容 |
|---|---|
| Windows のみ実行可 | 非 Windows でもビルドは通る (スタブ実装) が、`run` 等は OS エラー |
| `windows` クレート 0.58 採用 | 0.62 が最新だが API 探索を 0.58 ベースで完結したため固定 |
| `chrono` の `default-features=false` | `oldtime`/serde/wasmbind を切り、`clock` + `std` のみ。 サイズ最小化 |
| `panic = "abort"` | デバッグ時に panic backtrace の情報量が減る点に注意 |

## 10.8 リリース成果物の検証

最低限の検証 (リリース前):
1. `cargo build --release` が成功する。
2. `target/release/dmm-game-bot.exe` のサイズが概ね 6 MB 前後 (大幅増減があれば依存追加の見直し)。
3. `cargo test` で 9 件のユニットテストが全て通る (霊晶石ガード回帰防止)。
   CoordCache (DESIGN/11) 実装後は計 20 件。詳細は [`09-testing.md`](09-testing.md) §9.1.1。
4. `cargo clippy` で警告ゼロ。
5. `dmm-game-bot.exe -V` でバージョンが出る。
6. `dmm-game-bot.exe -v detect-once` で 9 種テンプレの読み込みログが INFO で出る。
