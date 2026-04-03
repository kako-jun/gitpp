# gitpp 仕様

最終更新: 2026-04-02

## コマンド

| コマンド | ショートカット | 動作 |
|---|---|---|
| `clone` | `clo`, `cl` | 全 enabled リポジトリを並列 clone |
| `pull` | `pul`, `pu` | 全 enabled リポジトリを並列 pull |
| `push` | `pus`, `ps` | 全 enabled リポジトリを並列 add → commit → push |
| `help` | `?` | コマンド一覧を表示（TUI なし） |

### オプション

| オプション | 説明 |
|---|---|
| `-c PATH` / `--config PATH` | 設定ファイルのパスを指定（デフォルト: CWD の `gitpp.yaml` / `gitpp.yml`） |
| `-r PATH` / `--root PATH` | リポジトリ展開先のルートディレクトリを指定（デフォルト: CWD） |
| `-j N` / `--jobs N` | 並列度の上限（デフォルト: gitpp.yaml の `jobs`、未指定なら 20） |

`-c`, `-r`, `-j` はグローバルオプション（コマンドの前でも後でも指定可能）。

### 各コマンドが実行する git 操作

| コマンド | 実行される git コマンド列 | 備考 |
|---|---|---|
| clone | `git clone <remote> -b <branch>` | group ディレクトリ内に実行 |
| pull | `git pull` | コンフリクト時は自動解決しない（Failed 扱い） |
| push | `git add -A` → `git commit -m "<msg>"` → `git push` | コミットメッセージは `comments.default` 固定 |

全操作の前後で YAML に書かれた user.name / user.email が各リポの `.git/config` に自動適用される。

### clone の重複検出

clone 先ディレクトリに `.git` が既に存在する場合、`git remote get-url origin` で実際の remote URL を取得し、YAML の remote と比較する。

| 状況 | 結果 |
|---|---|
| `.git` なし | 通常通り clone を実行 |
| `.git` あり + remote 一致 | "Already cloned" 表示（Success）。user config だけ適用 |
| `.git` あり + remote 不一致 | "Remote mismatch" 表示（Failed）。期待値と実際の remote を出力 |

## 動作モード

| モード | 起動方法 | 概要 |
|---|---|---|
| ワンショット | `gitpp <command>` | コマンドを1回実行して終了 |
| インタラクティブ | `gitpp`（引数なし） | REPL で繰り返しコマンドを実行 |

### インタラクティブモード

- プロンプト: `gitpp> `（シアン太字）
- Tab 補完: `clone`, `pull`, `push`, `help`, `exit`, `quit`
- ヒント: 入力中に候補をインライン表示（前方一致）
- 履歴: `~/.gitpp_history` に保存
- 終了: `exit` / `quit` / Ctrl+D

## 設定ファイル

### 検索ロジック

1. `--config` が指定されていればそのパスを使う
2. 指定がなければカレントディレクトリの `gitpp.yaml` を探す
3. なければ `gitpp.yml` を探す
4. どちらもなければエラー終了

### フォーマット

```yaml
user:
  name: <string>          # git config user.name
  email: <string>         # git config user.email
comments:
  default: <string>       # push 時のコミットメッセージ
jobs: <number>            # 並列実行数の上限（省略時 20）
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
| `jobs` | usize | no | 並列実行数の上限。CLI `-j` で上書き可。デフォルト 20 |
| `repos[].enabled` | bool | yes | false で対象から除外 |
| `repos[].remote` | String | yes | SSH or HTTPS のリモート URL |
| `repos[].branch` | String | yes | clone 時に `-b` で指定するブランチ |
| `repos[].group` | String | yes | `{group}/{repo_name}` のディレクトリに clone |

リポジトリ名は `remote` URL の末尾パス要素から自動抽出される（`.git` は除去）。

## TUI

ratatui + crossterm によるフルスクリーン TUI。

### レイアウト

**一覧モード（デフォルト）:**

```
┌──────────────────────────────────────────────────────────────┐
│ gitpp  j/k:move  Enter:detail  h/l:scroll  q:quit           │
└──────────────────────────────────────────────────────────────┘
┌─ Repositories [1-20/101] ────────────────────────────────────┐
│▸✓ freeza                           Done                     │
│  [████████████████████████████████████████] 100%             │
│ ⚙ sss                              Pulling...               │
│  [████████████████████░░░░░░░░░░░░░░░░░░░░]  50%            │
│ ⏸ noun-gender                       Waiting...              │
│  [░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░]   0%           │
└──────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ Total: 101 | Done: 50 | OK: 48 | Fail: 2                   │
└──────────────────────────────────────────────────────────────┘
```

**詳細モード（Enter で展開）:**

```
┌──────────────────────────────────────────────────────────────┐
│ gitpp  j/k:move  Enter:detail  h/l:scroll  q:quit           │
└──────────────────────────────────────────────────────────────┘
┌─ Repositories [1-20/101] ──────┬─ sss ───────────────────────┐
│   ✓ freeza         Done  100% │ remote: Enumerating objects:  │
│▸⚙ sss           Pull..   50% │   12, done.                  │
│   ⏸ noun-gender   Wait..   0% │ Receiving objects:  60%      │
│                                │   (7/12) 1.2 MiB            │
└────────────────────────────────┴──────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ Total: 101 | Done: 50 | OK: 48 | Fail: 2                   │
└──────────────────────────────────────────────────────────────┘
```

### キー操作

| キー | 動作 |
|---|---|
| j / k / ↑ / ↓ | リポ選択・スクロール |
| g | 先頭へ |
| G | 末尾へ |
| Enter | 詳細ペイン表示/非表示 |
| h / l / ← / → | 詳細ペイン内を縦スクロール（3行ずつ） |
| Esc | 詳細ペインを閉じる（ペイン非表示時はブラウズモード終了） |
| q | 強制終了 |

### 完了後の動作

全リポジトリの処理が完了すると:
1. **2秒間**キー操作を待つ
2. 操作なし → 自動終了し、stdout にサマリーを出力
3. 何かキーを押す → ブラウズモードに移行（j/k で結果を見回せる、q または Esc で終了）

実行中でも `q` を押せばいつでも即時終了できる。

### ステータス遷移

| ステータス | アイコン | 色 | 意味 |
|---|---|---|---|
| Pending | ⏸ | DarkGray | セマフォ待ち含む待機中 |
| Running | ⚙ | Yellow | 実行中 |
| Success | ✓ | Green | 完了 |
| Failed | ✗ | Red | 失敗 |

### エラー判定

git コマンドの exit code で判定。非ゼロなら Failed。

push 時は add → commit → push を順に実行し、途中で失敗したら以降をスキップする。
`git commit` が "nothing to commit" で非ゼロ終了した場合は正常扱いとし、push もスキップして Success を返す（変更がないのに push する無駄を避ける）。
この判定のみ exit code + 出力文字列の複合判定を使う。

### TUI 終了後のサマリー出力

TUI 終了後、プレーンテキスト（ANSI エスケープコードなし）でサマリーを stdout に出力する。
そのままクリップボードにコピーして AI エージェント等に貼り付けられる形式。

**全成功時:**
```
gitpp pull: all 101 repositories succeeded.
```

**失敗あり:**
```
gitpp pull: 98/101 succeeded, 3 failed

--- freeza (/Users/kako-jun/repos/private/freeza) ---
  error: Your local changes to the following files would be overwritten by merge:
    src/main.rs

--- sss (/Users/kako-jun/repos/2025/sss) ---
  fatal: refusing to merge unrelated histories
```

各失敗リポについて、リポ名・フルパス・git 出力を表示する。
push の場合は add → commit → push 全ステップの出力が結合される。

## 並列実行

- 1リポジトリ = 1スレッド（`std::thread`）
- セマフォ（`Mutex<usize>` + `Condvar`）で同時実行数を制限
- デフォルト並列度: `jobs` 設定値（未指定時 20）
- CLI `-j N` で上書き可能
- 共有データ: `Arc<Mutex<Vec<RepoProgress>>>`
- TUI は 100ms ごとにポーリングして画面更新

## OS 対応

| OS | 文字コード | 備考 |
|---|---|---|
| Windows | Shift_JIS | git 出力のデコードに使用 |
| Linux / macOS | UTF-8 | |

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
