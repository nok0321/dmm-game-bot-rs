# 設計書 (DESIGN)

`dmm-game-bot` (DMM ブラウザゲーム自動化ツール) の設計ドキュメント群です。
本ドキュメントは **2026-04-25 時点の動作するベータ版実装** に対応しています
(2 サイクル継続成立を実機で確認済み)。

設計の起点は [`../templates/dmm-game-bot_architecture.md`](../templates/dmm-game-bot_architecture.md)
v1.1 ですが、ベータ版完成までに追加・変更された下記要素は v1.1 には未反映のため、
**現行実装と整合する DESIGN/ 配下の文書を真** として扱ってください。

## v1.1 → 現行ベータ間の主な差分

- **ハード sleep 戦略**:
  `post_battle_min_wait_ms` / `next1_settle_wait_ms` / `next2_settle_wait_ms` を導入。
  戦闘演出・モーダルアニメ中の偽マッチを「画面を見ない時間」で根本回避。
- **debounce 方式を放棄**:
  `wait_template_gone` / `BotError::TemplateGoneTimeout` を削除し、
  リザルト系の連続画面は遷移待ちハード sleep に統一。
- **Stability check** (`stability_count` / `stability_position_tol_px` / `stability_score_tol`)
  を追加: フェードイン中の半透明ボタンへの誤クリックを抑止。
- **ログタイムスタンプを JST (+09:00) 固定**: システムロケール非依存。
- **CLI に `--live` / `--post-battle-min-wait-ms` を追加**:
  dry_run の明示的解除と動作確認用の戦闘待ち短縮。
- **Config validation で霊晶石ガードの最小 threshold (0.80) と必須 ROI を強制**:
  「緩める方向の事故設定」を起動時に弾く。
- **PrintWindowCapturer の取得範囲をウィンドウ全体に変更 → クライアント領域でクロップ**:
  Chrome の GPU 描画でも安定させる。失敗時は BitBlt フォールバック。
- **入力エミュレーションに `MOUSEEVENTF_VIRTUALDESK` を追加**:
  マルチモニタ環境での座標補正を堅牢化。
- **`OnMiss::Skip`**: Step 9 (Close) のタイムアウトは `StepLog.skipped=true` で正常スキップ扱い。

## 章立て

| ファイル | 内容 |
|---|---|
| [01-overview-and-scope.md](01-overview-and-scope.md) | 目的、自動化対象シーケンス、スコープ、非機能要件 |
| [02-architecture.md](02-architecture.md) | レイヤー構造、モジュール構成、採用クレート |
| [03-runtime-flow.md](03-runtime-flow.md) | 9 ステップ実行フロー、stability check、待機戦略、OnMiss |
| [04-vision-and-templates.md](04-vision-and-templates.md) | テンプレートマッチング (NCC) と ROI 限定 |
| [05-platform-windows.md](05-platform-windows.md) | ウィンドウ列挙・キャプチャ・入力・DPI |
| [06-config-schema.md](06-config-schema.md) | TOML スキーマ、デフォルト値、起動時バリデーション |
| [07-cli.md](07-cli.md) | サブコマンドとフラグ |
| [08-safety-and-errors.md](08-safety-and-errors.md) | 霊晶石ガード、エラー型、ドライラン、デイリー停止 |
| [09-testing.md](09-testing.md) | 現行ユニットテストと未着手の統合テスト計画 |
| [10-build.md](10-build.md) | リリースビルド、依存、配布構成 |

## 関連ドキュメント

- [`../CHECKPOINT.md`](../CHECKPOINT.md): セッション継続用の作業メモ (進行中課題と申し送り)
- [`../templates/README.md`](../templates/README.md): テンプレート画像セットの仕様
- [`../templates/dmm-game-bot_architecture.md`](../templates/dmm-game-bot_architecture.md): v1.1 設計書 (歴史的参考)
