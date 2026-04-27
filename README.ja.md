# gitpp

[English](README.md)

[![CI](https://github.com/kako-jun/gitpp/actions/workflows/ci.yml/badge.svg)](https://github.com/kako-jun/gitpp/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/gitpp.svg)](https://crates.io/crates/gitpp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Git Personal Parallel Manager — 100以上のGitリポジトリを1コマンドで管理する。

## なぜ gitpp なのか

100以上のリポジトリを複数のマシンで管理していると、作業開始前の手順が毎回同じになる：

```bash
# gitpp なし — リポジトリの数だけ繰り返す
cd ~/repos/private/project-a && git pull
cd ~/repos/private/project-b && git pull
cd ~/repos/2025/tool-x       && git pull
# ... あと97回
```

gitpp なら — 1コマンド、全リポジトリを並列処理、ライブTUI付き：

```bash
gitpp pull
```

それだけではない。gitpp はもう一つの問題も解決する：**コミットユーザーの誤爆**。

個人の OSS リポジトリ、仕事のリポジトリ、趣味のサイドプロジェクトを同じマシンに置いている場合、`~/.gitconfig` に `user.name`/`user.email` を書くと「1つのアイデンティティが勝ち残り、他は誤った帰属になる」問題が起きる。

gitpp は逆の発想をとる。`~/.gitconfig` のグローバル設定には依存せず、YAMLファイルの `config:` セクションに書いた内容を `git config --local` でリポジトリごとに設定する。アイデンティティはグローバル設定ではなく「YAMLを置いた場所」に紐づく。

## 機能

- **clone / pull / push を並列実行**（並列度は `jobs` で設定、デフォルト20）
- **status / diff / fetch / branch / switch / stash list / gc** — リポジトリ横断の一括操作
- **フルスクリーンTUI**（ratatui）— 6状態表示（Waiting/Running/Updated/Unchanged/Failed/Untracked）とリアルタイムプログレス
- **場所ごとに git config を分離** — `user.name`, `pull.rebase` など任意の git config キーをグループ内の全リポジトリにローカル設定
- **push はオプトイン制** — `comments.default` を明示的に設定しない限り push は無効。clone/pull はそれなしで動く
- **AIエージェント向けサマリー** — 完了後にプレーンテキストで結果を stdout に出力。そのままAIに貼り付けられる
- **端末の後始末を強化** — terminal mouse capture を有効化しないため、終了時にマウスイベント断片がシェルへ漏れない
- **インタラクティブREPL** モード（タブ補完・履歴付き）
- **クワイエットモード** — TUIなし、スクリプトやCI向け
- **pre-commit hook 失敗時の自動リトライ**（1回）
- シングルバイナリ、ランタイム依存なし

## できること

リポジトリ群のルートディレクトリに `gitpp.yaml` を置き、サブコマンドを実行する：

| コマンド | 短縮形 | 実行内容 |
|----------|--------|----------|
| `clone` | — | 全リポジトリを並列 clone |
| `pull` | — | 全リポジトリを並列 pull |
| `push` | — | 全リポジトリを並列 add → commit → push |
| `status` | `st` | `git status --porcelain`（未コミット変更の検出） |
| `diff` | `di` | `git diff --stat HEAD`（staged+unstaged差分） |
| `fetch` | `fe` | `git fetch`（リモート状態取得） |
| `branch` | `br` | 現在のブランチ表示（main/master以外を強調） |
| `switch` | `sw` | デフォルトブランチに切替（Git 2.23+必要） |
| `stash list` | `sl` | 忘れられた stash の検出 |
| `gc` | — | ガベージコレクション（I/Oヘビー、`-j` で並列数制限推奨） |

例：

```bash
gitpp clone        # 全リポジトリを並列 clone
gitpp pull         # 全リポジトリを並列 pull
gitpp push         # 全リポジトリを並列 add → commit → push
gitpp status       # 未コミット変更の検出
gitpp diff         # staged+unstaged の差分サマリー
gitpp fetch        # リモート状態を取得
gitpp branch       # 現在のブランチ表示（main/master以外を強調）
gitpp switch       # デフォルトブランチに切替（Git 2.23+）
gitpp stash list   # 忘れられた stash を検出
gitpp gc           # ガベージコレクション（-j で並列数制限推奨）
```

フルスクリーンTUIがすべてのリポジトリの進捗をリアルタイムで表示する：

**一覧モード（デフォルト）:**

```
┌──────────────────────────────────────────────────────────────┐
│ gitpp  j/k:move  Enter:detail  g/G:top/end  n/N:next error  │
│ h/l:scroll  y:copy  Esc:back  q:quit                        │
└──────────────────────────────────────────────────────────────┘
┌─ Repositories [1-20/101] ────────────────────────────────────┐
│▸✓ freeza                           Updated                  │
│  [████████████████████████████████████████] 100%             │
│ ▶ sss                              Pulling...               │
│  [████████████████████░░░░░░░░░░░░░░░░░░░░]  50%            │
│ ⏸ noun-gender                       Waiting...              │
│  [░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░]   0%           │
└──────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ Total: 103 | Done: 52 (Updated: 30 / Unchanged: 18 / Failed: 2 / Untracked: 2) │
└──────────────────────────────────────────────────────────────┘
```

`base_dir` 配下に存在するが `gitpp.yaml` に定義されていないリポジトリは **Untracked**（`?` アイコン、マゼンタ）として表示される。手動 clone したリポや yaml への追加忘れに気づける。

任意のリポジトリで Enter を押すと詳細ペインが開き、git のリアル出力を確認できる：

**詳細モード:**

```
┌──────────────────────────────────────────────────────────────┐
│ gitpp  j/k:move  Enter:detail  g/G:top/end  n/N:next error  │
│ h/l:scroll  y:copy  Esc:back  q:quit                        │
└──────────────────────────────────────────────────────────────┘
┌─ Repositories [1-20/101] ──────┬─ sss ───────────────────────┐
│   ✓ freeza         Done  100% │ remote: Enumerating objects:  │
│▸▶ sss           Pull..   50% │   12, done.                  │
│   ⏸ noun-gender   Wait..   0% │ Receiving objects:  60%      │
│                                │   (7/12) 1.2 MiB            │
└────────────────────────────────┴──────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ Total: 103 | Done: 52 (Updated: 30 / Unchanged: 18 / Failed: 2 / Untracked: 2) │
└──────────────────────────────────────────────────────────────┘
```

全リポジトリの処理が完了すると、プレーンテキストのサマリーを stdout に出力する。そのままAIアシスタントに貼り付けて診断依頼できる：

```
gitpp pull: 98/101 succeeded, 3 failed

--- freeza (/Users/you/repos/private/freeza) ---
  error: Your local changes to the following files would be overwritten by merge:
    src/main.rs

--- sss (/Users/you/repos/2025/sss) ---
  fatal: refusing to merge unrelated histories
```

## インストール

```bash
cargo install gitpp
```

ソースからビルドする場合：

```bash
git clone https://github.com/kako-jun/gitpp.git
cd gitpp
cargo install --path .
```

## 設定

リポジトリ群のルートに `gitpp.yaml` を作成する：

```yaml
config:
  user.name: your-name
  user.email: your-email@example.com
  pull.rebase: "true"
comments:
  default: sync.    # push 時の固定コミットメッセージ
jobs: 20
repos:
  - enabled: true
    remote: git@github.com:user/repo-a.git
    branch: main
    group: "projects"
  - enabled: true
    remote: git@github.com:user/repo-b.git
    branch: main
    group: "projects"
  - enabled: false             # このリポジトリはスキップ
    remote: git@github.com:user/archived.git
    branch: main
    group: "archive"
```

| フィールド | 型 | 必須 | 説明 |
|-----------|-----|------|------|
| `config` | map | 任意 | `git config --local` に渡す任意の key-value。`user.name`, `pull.rebase`, `core.autocrlf` など任意の git config キーを指定可能。各操作の前後でリポジトリごとに適用される。YAMLからキーを削除しても既存リポの `.git/config` からは自動削除されない（上書きのみ）。 |
| `comments.default` | 文字列 | 任意* | push 時の固定コミットメッセージ。**空または未設定の場合 push は無効。** clone/pull だけ使うなら `comments` セクション自体を省略してよい。 |
| `jobs` | 数値 | 任意 | 同時実行数の上限。デフォルト: 20。CLI の `-j N` で上書き可。 |
| `repos[].enabled` | bool | 必須 | `false` でリポジトリをスキップ（ファイルから削除せず無効化できる）。 |
| `repos[].remote` | 文字列 | 必須 | SSH または HTTPS のリモートURL。リポジトリ名はURLの末尾から自動抽出（`.git` を除去）。 |
| `repos[].branch` | 文字列 | 必須 | clone 時に `-b` で渡すブランチ名。 |
| `repos[].group` | 文字列 | 必須 | clone 先のサブディレクトリ名。`group: "2025"` なら `./2025/repo-name` に clone される。 |

### 1台のマシンで複数のアイデンティティを使い分ける

ディレクトリごとにYAMLファイルを用意し、それぞれ別の `config:` を書く：

```
~/repos/
  personal/
    gitpp.yaml   # user.email: me@personal.dev
  work/
    gitpp.yaml   # user.email: me@company.com
  hobby/
    gitpp.yaml   # user.email: me@hobbyaccount.io
```

`~/.gitconfig` にグローバルな user 設定は不要。むしろ設定しないことがフェイルセーフになる。ローカル設定のないリポジトリはコミット時にエラーになるため、誤爆を事前に防げる。

## 使い方

```bash
# ワンショットモード
gitpp pull              # 全 enabled リポジトリを pull
gitpp push -j 10        # 最大10並列で push
gitpp clone             # clone（既に clone 済みのリポジトリはスキップ）

# v0.6.4 で追加されたサブコマンド
gitpp status            # 未コミット変更の検出（短縮形: st）
gitpp diff              # staged+unstaged の差分サマリー（短縮形: di）
gitpp fetch             # リモート状態を取得（短縮形: fe）
gitpp branch            # 現在のブランチ表示（短縮形: br）
gitpp switch            # デフォルトブランチに切替（短縮形: sw）
gitpp stash list        # 忘れられた stash を検出（短縮形: sl）
gitpp gc                # ガベージコレクション（I/Oヘビー、-j で並列数制限推奨）

# 設定ファイルの場所を指定
gitpp pull --config ~/shared/gitpp.yaml

# 設定ファイルとリポジトリルートを個別に指定
gitpp clone -c /mnt/ssd/gitpp.yaml -r /mnt/ssd/repos

# クワイエットモード（TUIなし — サマリーは stdout、進捗は stderr）
gitpp pull -q

# ヘルプ / バージョン確認
gitpp --help            # or: gitpp -h
gitpp --version         # or: gitpp -V

# インタラクティブモード（タブ補完付き）
gitpp
gitpp> pull
gitpp> exit
```

### TUI 操作

| キー | 動作 |
|------|------|
| `j` / `k` / `↑` / `↓` | リポジトリを選択・スクロール |
| `g` / `G` | 先頭 / 末尾へ移動 |
| `n` / `N` | 次 / 前のエラーにジャンプ |
| `Enter` | 詳細ペインの表示/非表示 |
| `h` / `l` / `←` / `→` | 詳細ペイン内をスクロール（3行ずつ） |
| `y` | 選択中リポジトリの出力をクリップボードにコピー |
| `Esc` | 段階終了（詳細ペインを閉じる → ブラウズモード終了） |
| `q` | 即時終了 |

全リポジトリの処理完了後、3秒間キー入力を待つ（フッタにヒント表示）。何かキーを押すとブラウズモードに移行して結果を確認できる。何もしなければ自動終了する。

**ステータスアイコン:**

| アイコン | 状態 | 色 |
|----------|------|-----|
| `⏸` | Waiting | — |
| `▶` | Running | — |
| `✓` | Updated | 緑 |
| `─` | Unchanged | グレー |
| `✗` | Failed | 赤 |

### clone の重複検出

clone 先に既に `.git` が存在する場合：

| 状況 | 結果 |
|------|------|
| `.git` なし | 通常通り clone を実行 |
| `.git` あり、remote 一致 | "Already cloned"（Success）— config のみ適用 |
| `.git` あり、remote 不一致 | "Remote mismatch"（Failed）— 期待値と実際の remote を出力 |

## ライセンス

MIT
