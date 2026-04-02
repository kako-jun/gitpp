# gitpp - Git Personal Parallel Manager

## 概要

**gitpp** は、複数のGitリポジトリを一括で管理するツール。YAML設定ファイルに基づいて clone / pull / push を並列実行する。ratatui ベースの TUI でリアルタイム進捗を表示。

元は `gitp.sh`（Bash版）を Rust で再実装したもの。crates.io 公開時の名前は `gitpp`（git + personal + parallel）。

## コマンド

3つだけ:
- `gitpp clone` — 全リポジトリをクローン
- `gitpp pull` — 全リポジトリをプル
- `gitpp push` — 全リポジトリを add -A → commit → push

引数なしで起動するとインタラクティブモード（Tab補完・履歴あり）。

### オプション

- `-j N` / `--jobs N` — 並列度の上限（デフォルト: gitpp.yaml の `jobs`、未指定なら20）

### ショートカット

`clo`/`cl` → clone、`pul`/`pu` → pull、`pus`/`ps` → push

## 設定ファイル: gitpp.yaml

```yaml
user:
  name: kako-jun
  email: 3541096+kako-jun@users.noreply.github.com
comments:
  default: update.
jobs: 20
repos:
  - enabled: true
    remote: git@github.com:kako-jun/gitpp.git
    branch: main
    group: "2025"
```

- `.yaml` と `.yml` の両方をサポート（`.yaml` 優先）
- `jobs` — 並列実行数の上限。CLI の `-j` で上書き可能
- `user.name` / `user.email` は clone 直後、pull/push 前に各リポジトリの `.git/config` に自動設定
- `comments.default` — push 時のコミットメッセージ（固定）
- `group` — リポジトリの配置先ディレクトリ

## TUI

ratatui + crossterm によるフルスクリーン TUI。

### 操作

| キー | 動作 |
|---|---|
| j / ↓ | 次のリポジトリ |
| k / ↑ | 前のリポジトリ |
| g | 先頭へ |
| G | 末尾へ |
| Enter | 詳細ペイン表示/非表示（右半分にgit出力） |
| h / l / ← / → | 詳細ペイン内スクロール |
| Esc | 詳細ペインを閉じる（ペイン非表示時はブラウズモード終了） |
| q | 強制終了 |

### 表示

- ⏸ グレー: 待機中（セマフォ待ち含む）
- ⚙ 黄色: 実行中
- ✓ 緑: 成功
- ✗ 赤: 失敗
- フッター: Total / Done / OK / Fail の集計 + キーバインド表示
- スクロール: 100+ リポジトリに対応、タイトルバーに `[1-20/101]` 表示

### 終了後サマリー

TUI 終了後、失敗したリポジトリの git 出力を stdout に表示。全成功なら "All N repositories succeeded." と表示。

## アーキテクチャ

```
gitpp/
├── Cargo.toml
├── gitpp.yaml          # 設定ファイル
├── CLAUDE.md
├── README.md
├── LICENSE
├── docs/
│   ├── overview.md
│   ├── spec.md
│   └── roadmap.md
└── src/
    ├── main.rs           # CLI パース、セマフォ、ワーカースレッド管理
    ├── interactive.rs     # インタラクティブモード (rustyline)
    ├── git_controller.rs  # Git 操作の実行
    ├── setting_util.rs    # gitpp.yaml の読み込み
    └── tui.rs             # ratatui TUI（スクロール、詳細ペイン、サマリー）
```

### 並列度制御

`Semaphore`（`Mutex<usize>` + `Condvar`）で同時実行スレッド数を制限。全スレッドは起動されるが、セマフォの acquire で待機する。TUI 上では Pending 表示。

## 依存関係

```toml
encoding_rs = "0.8.34"     # 文字コード変換（Windows SJIS対応）
serde + serde_yaml          # YAML 解析
ratatui = "0.28"            # TUI フレームワーク
crossterm = "0.28"          # ターミナル制御
rustyline = "14.0"          # インタラクティブモード
dirs = "5.0"                # ホームディレクトリ取得
```

## ビルドと実行

```bash
cargo build --release
cargo install --path .

# ワンショット
gitpp pull
gitpp push -j 10

# インタラクティブ
gitpp
gitpp> pull
gitpp> exit
```
