# gitpp ロードマップ

最終更新: 2026-04-02

## リブート方針

gitp.sh の原型に立ち返り、Rust 版をシンプルに再構築する。
機能を足すのではなく削ぎ落とす。

crate 名・リポ名・バイナリ名はすべて **`gitpp`** に統一済み。

## 完了済み

### Phase 1: コード修正（2026-04-02 セッション120）

- [x] `config` / `config user` コマンド削除
- [x] `serial` オプション削除
- [x] macOS 判定修正（"unknown OS" → UTF-8 統合）
- [x] デバッグ println 削除
- [x] YAML パースエラー修正（空設定でエラー終了）
- [x] `git_status()` / `git_config_raw()` 削除
- [x] 設定ファイル名 `gitp_setting.yaml` → `gitpp.yaml`
- [x] リポ名・フォルダ名 `gitp` → `gitpp`
- [x] GitHub リポ名 `kako-jun/gitp` → `kako-jun/gitpp`
- [x] 並列度制限追加（セマフォ、`-j N` / `--jobs N`、YAML `jobs: 20`）

### Phase 2: TUI 改修（2026-04-02 セッション120）

- [x] 縦スクロール（100+ リポ対応、タイトルに `[1-20/101]` 表示）
- [x] 右ペイン詳細表示（Enter で git 出力をリアルタイム表示）
- [x] キー操作（j/k/↑/↓ スクロール、g/G 先頭/末尾、Enter 詳細、Esc 閉じる）
- [x] 選択カーソル（▸ 表示、反転スタイル）
- [x] TUI 終了後サマリー（失敗リポの出力を stdout に表示）
- [x] 完了後ブラウズモード（自動終了前にキー操作で結果を確認可能）

## 残タスク

### Phase 3: 動作確認

- [ ] gitp.sh と並行して pull を実行し、結果を比較
- [ ] 101 リポジトリ全体で動作確認

### Phase 4: 移行

- [ ] `cargo install` で配布テスト
- [ ] gitp.sh 廃止
- [ ] crates.io 公開（`gitpp`）

### 将来検討

- README.md の整備（公開前）
- gitp_config.yaml → gitpp.yaml の変換スクリプト（Bash版ユーザー向け）
