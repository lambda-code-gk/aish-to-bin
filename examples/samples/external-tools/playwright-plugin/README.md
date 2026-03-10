# AISH 外部ツールプラグイン（Playwright サンプル）

Playwright を使ったブラウザ操作を AISH のツールとして提供するサンプルプラグインです。

## 提供ツール

| ツール名 | 説明 |
|----------|------|
| `browser_launch` | Chromium を起動する（最初に呼ぶ） |
| `browser_goto` | 指定 URL を開く |
| `browser_content` | 現在のページの本文テキストを取得 |
| `browser_click` | CSS セレクタで指定した要素をクリック |
| `browser_fill` | 入力欄に文字を入力 |
| `browser_screenshot` | ページのスクリーンショットを保存 |
| `browser_close` | ブラウザを閉じる |

## セットアップ

### 1. 依存関係のインストール

```bash
cd examples/samples/external-tools/playwright-plugin
npm install
```

初回は Playwright が Chromium をダウンロードするため、少し時間がかかります。

### 2. manifest を探索ディレクトリに置く（正式: TOML）

**推奨**: `plugin.toml.example` をコピーし、`args` を **このリポジトリ内の `plugin.mjs` の絶対パス** に書き換えてから、次のいずれかに配置する。

- **プロジェクトで使う場合**: `{project_root}/.aish/plugins/playwright/plugin.toml`
- **ユーザー全体**: `config/plugins/playwright/plugin.toml`（`$AISH_HOME/config/plugins` または `$XDG_CONFIG_HOME/aish/plugins`）
- **レガシー**: `~/.aish/plugins.d/playwright.toml`

```bash
# 例: プロジェクトの .aish/plugins に置く場合（リポジトリルートを /home/user/aish_to_bin とする）
mkdir -p .aish/plugins/playwright
cp plugin.toml.example .aish/plugins/playwright/plugin.toml
# .aish/plugins/playwright/plugin.toml の args を /home/user/aish_to_bin/examples/samples/external-tools/playwright-plugin/plugin.mjs に書き換える
```

TOML では **`enabled = true`** を明示しないとプラグインは有効にならない（deny-by-default）。

### Legacy YAML を使う場合

`manifest.yaml.example` は旧形式のサンプルです。同じ探索場所（`config/plugins.d` や `~/.aish/plugins.d` など）に `*.yaml` を置けば読み込まれる。YAML でも **`enabled: true`** を明示すること。

### 3. AISH (ai) の起動

```bash
# プロジェクトルートから
./dist/bin/ai "リストのトップの見出しを教えて" -s /tmp/demo-session
```

プラグインが読み込まれていれば、`browser_launch` や `browser_goto` がツール一覧に現れ、LLM がそれらを呼び出せます。

## 使用例（LLM に任せる場合）

- 「https://example.com を開いて、ページの内容を要約して」
- 「Google を開いて「Playwright」で検索し、最初のリンクのタイトルを教えて」

最初に `browser_launch`、次に `browser_goto` で URL を開き、その後 `browser_content` や `browser_click` / `browser_fill` を組み合わせる流れになります。

## 注意

- プラグインは **探索ディレクトリ**（プロジェクトの `.aish/plugins`、config の `plugins` / `plugins.d`、`~/.aish/plugins.d`）に置いた manifest からのみ有効です。詳細は [docs/external-tools.md](../../../docs/external-tools.md) を参照。
- ヘッドレスでないブラウザ（`browser_launch` で `headless: false`）は、GUI がある環境でのみ利用してください。

## 参考

- [docs/external-tools.md](../../../docs/external-tools.md) — 外部ツールの仕様、探索場所、plugin.toml 形式
