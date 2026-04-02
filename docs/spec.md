# gitp 仕様

最終更新: 2026-04-02

## コマンド

| コマンド | ショートカット | 動作 |
|---|---|---|
| `clone` | `clo`, `cl` | 全 enabled リポジトリを並列 clone |
| `pull` | `pul`, `pu` | 全 enabled リポジトリを並列 pull |
| `push` | `pus`, `ps` | 全 enabled リポジトリを並列 add → commit → push |
| `help` | `?` | コマンド一覧を表示（TUI なし） |

実行は常に並列。serial オプションはない。

### 各コマンドが実行する git 操作

| コマンド | 実行される git コマンド列 | 備考 |
|---|---|---|
| clone | `git clone <remote> -b <branch>` | group ディレクトリに cd してから実行 |
| pull | `git pull` | コンフリクト時は自動解決しない（Failed 扱い） |
| push | `git add -A` → `git commit -m "<msg>"` → `git push` | コミットメッセージは `comments.default` 固定 |

全操作の前後で YAML に書かれた user / config が各リポの `.git/config` に自動適用される。

| 操作 | config 適用タイミング |
|---|---|
| clone | clone 直後 |
| pull | pull 直後 |
| push | push 直前 |

## 動作モード

| モード | 起動方法 | 概要 |
|---|---|---|
| ワンショット | `gitp <command>` | コマンドを1回実行して終了 |
| インタラクティブ | `gitp`（引数なし） | REPL で繰り返しコマンドを実行 |

### インタラクティブモード

- プロンプト: `gitp> `（シアン太字）
- Tab 補完: `clone`, `pull`, `push`, `help`, `exit`, `quit`
- ヒント: 入力中に候補をインライン表示（前方一致）
- 履歴: `~/.gitp_history` に保存
- 終了: `exit` / `quit` / Ctrl+D

## 設定ファイル

### 検索ロジック

1. カレントディレクトリの `gitp.yaml` を探す
2. なければ `gitp.yml` を探す
3. どちらもなければエラー終了

### フォーマット

```yaml
user:
  name: <string>          # git config user.name
  email: <string>         # git config user.email
comments:
  default: <string>       # push 時のコミットメッセージ
config:                   # 省略可。任意の git config キー
  <key>: <value>
repos:
  - enabled: <bool>       # false なら対象外
    remote: <string>      # git リモート URL
    branch: <string>      # clone 時のブランチ
    group: <string>       # clone 先サブディレクトリ名
```

### フィールド詳細

| フィールド | 型 | 必須 | 説明 |
|---|---|---|---|
| `user.name` | String | yes | 全リポジトリに設定する git user.name |
| `user.email` | String | yes | 全リポジトリに設定する git user.email |
| `comments.default` | String | yes | push 時の固定コミットメッセージ |
| `config` | Map | no | 任意の git config を key: value で列挙。git config のキー名をそのまま使える |
| `repos[].enabled` | bool | yes | false で対象から除外。環境ごとに一部だけ clone したい場合に使う |
| `repos[].remote` | String | yes | SSH or HTTPS のリモート URL |
| `repos[].branch` | String | yes | clone 時に `-b` で指定するブランチ |
| `repos[].group` | String | yes | `{group}/{repo_name}` のディレクトリに clone |

リポジトリ名は `remote` URL の末尾パス要素から自動抽出される（`.git` は除去）。

### gitp.yaml と gitp_config.yaml の関係

| | gitp.yaml | gitp_config.yaml |
|---|---|---|
| 使用ツール | gitp（Rust 版） | gitp.sh（Bash 版） |
| config セクション | あり | なし |
| repos[].name | なし（URL から自動抽出） | あり（明示指定） |

## TUI

ratatui + crossterm によるフルスクリーン TUI。

### レイアウト

2段階の情報構造。一覧で全体を把握し、詳細で個別の出力を確認する。

**一覧モード（デフォルト）:**

```
┌─ gitp pull ─────────────────── 87/100 done ┐
│                                             │
│ ✓ freeza              Done         100%    │
│ ✓ diffx               Done         100%    │
│ ⚙ sss                 Fetching...   60%    │
│   src/main.rs                              │
│ ⚙ xsg                 Resolving...  30%    │
│   frontend/src-tauri/src/lib.rs            │
│ ⏸ noun-gender          Waiting...    0%    │
│                                             │
└─────────────────────────────────────────────┘
```

- j/k or 矢印で縦スクロール（一覧・詳細とも）
- 実行中のリポに自動フォーカス（手動スクロールで解除）
- 100リポでも破綻しない

**詳細モード（Enter で展開）:**

```
┌─ gitp pull ──────────────── 87/100 ─┬─ sss (detail) ──────────────┐
│                                      │                             │
│   ✓ freeza            Done    100%  │ remote: Enumerating objects: │
│   ✓ diffx             Done    100%  │   12, done.                 │
│ > ⚙ sss            Fetch..    60%  │ remote: Counting objects:    │
│   ⚙ xsg            Resolv..   30%  │   100% (12/12), done.       │
│   ⏸ noun-gender     Wait..     0%  │ Receiving objects:  60%      │
│   ⏸ tail-match      Wait..     0%  │   (7/12) 1.2 MiB            │
│                                      │                             │
└──────────────────────────────────────┴─────────────────────────────┘
```

- 右ペインに選択中リポの stdout/stderr がリアルタイムで流れる
- Esc or q で右ペインを閉じて一覧に戻る

### ステータス遷移

| ステータス | アイコン | 色 | 意味 |
|---|---|---|---|
| Pending | ⏸ | DarkGray | 待機中 |
| Running | ⚙ | Yellow | 実行中 |
| Success | ✓ | Green | 完了 |
| Failed | ✗ | Red | 失敗 |

### キー操作

| キー | 動作 |
|---|---|
| j/k, 矢印 | リポ選択・スクロール |
| Enter | 詳細モード（右ペイン展開） |
| Esc | 詳細モードを閉じる |
| q | 即時終了 |
| 全リポ完了時 | 1秒表示して自動終了 |

### エラー判定

git コマンドの出力に `"fatal"` または `"error"` が含まれていれば Failed。

### TUI 終了後のサマリー出力

TUI 終了後、失敗リポの git 出力を stdout にそのまま流す。成功リポは出力しない。

```
[gitp] 3 failed:

--- diffx ---
error: Your local changes to the following files would be overwritten by merge:
  src/main.rs

--- sss ---
fatal: refusing to merge unrelated histories

--- xsg ---
CONFLICT (content): Merge conflict in src/lib.rs
```

## 並列実行

- 1リポジトリ = 1スレッド（`std::thread`）。上限なし
- 共有データ: `Arc<Mutex<Vec<RepoProgress>>>`
- TUI は 100ms ごとにポーリングして画面更新

## OS 対応

| OS | 文字コード | 備考 |
|---|---|---|
| Windows | Shift_JIS | git 出力のデコードに使用 |
| Linux | UTF-8 | |
| macOS | UTF-8 | |

## 技術スタック

| クレート | バージョン | 用途 |
|---|---|---|
| ratatui | 0.28 | TUI フレームワーク |
| crossterm | 0.28 | ターミナル制御 |
| rustyline | 14.0 | インタラクティブモード（REPL） |
| serde + serde_yaml | 1.0 / 0.9 | YAML 設定読み込み |
| encoding_rs | 0.8 | OS 別文字コード変換 |
| dirs | 5.0 | ホームディレクトリ取得 |

Rust edition 2021。
