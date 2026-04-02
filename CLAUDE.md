# gitpp - Git Personal Parallel Manager

複数の Git リポジトリを YAML 設定で一括 clone / pull / push するツール。
ratatui TUI でリアルタイム進捗表示。元は gitp.sh（Bash）の Rust 再実装。

## ドキュメント

| ファイル | 内容 |
|---|---|
| `docs/overview.md` | 設計思想、使用場面、競合比較 |
| `docs/spec.md` | コマンド仕様、TUI操作、設定ファイル形式、エラー判定、サマリー出力 |
| `docs/roadmap.md` | 完了済み・残タスク |
| `README.md` | エンドユーザー向けの使い方 |

## ソース構成

```
src/
├── main.rs           # CLI パース、セマフォ（RAII）、ワーカースレッド管理
├── git_controller.rs  # Git 操作（GitResult で exit code + 出力を返す）
├── setting_util.rs    # gitpp.yaml の読み込み
├── interactive.rs     # インタラクティブモード（rustyline、Tab補完）
└── tui.rs             # ratatui TUI（スクロール、詳細ペイン、サマリー出力）
```

## 主要な設計判断

- **CWD は使わない** — 全 git コマンドは `Command::current_dir()` でディレクトリ指定（スレッド安全）
- **エラー判定は exit code** — "nothing to commit" のみ出力文字列との複合判定
- **clone 重複検出** — `.git` + `git remote get-url origin` で remote URL 一致を確認
- **セマフォ + mutex は poison 耐性** — `unwrap_or_else(|e| e.into_inner())`
- **サマリーは ANSI なしプレーンテキスト** — コピペで AI エージェントに渡せる形式
