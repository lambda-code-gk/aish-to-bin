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
cd samples/external-tools/playwright-plugin
npm install
```

初回は Playwright が Chromium をダウンロードするため、少し時間がかかります。

### 2. manifest を信頼ディレクトリに置く

manifest の `transport.args` を、**このリポジトリ内の `plugin.mjs` の絶対パス**に書き換えてからコピーします。

```bash
# 例: リポジトリのルートを /home/user/aish_to_bin とする場合
mkdir -p ~/.aish/plugins.d
cat > ~/.aish/plugins.d/playwright.yaml << 'EOF'
id: playwright
version: "0.1.0"
transport:
  type: stdio
  command: node
  args:
    - "/home/user/aish_to_bin/samples/external-tools/playwright-plugin/plugin.mjs"
timeouts:
  startup_ms: 15000
  call_ms: 60000
enabled: true
EOF
```

別の信頼ディレクトリを使う場合は `~/.config/aish/plugins.d/` に置いても動作します（XDG / AISH_HOME の config に依存）。

### 3. AISH (ai) の起動

```bash
# プロジェクトルートから
./core/ai/target/debug/ai "リストのトップの見出しを教えて" -s /tmp/demo-session
```

プラグインが読み込まれていれば、`browser_launch` や `browser_goto` がツール一覧に現れ、LLM がそれらを呼び出せます。

## 使用例（LLM に任せる場合）

- 「https://example.com を開いて、ページの内容を要約して」
- 「Google を開いて「Playwright」で検索し、最初のリンクのタイトルを教えて」

最初に `browser_launch`、次に `browser_goto` で URL を開き、その後 `browser_content` や `browser_click` / `browser_fill` を組み合わせる流れになります。

## 注意

- プラグインは **信頼ディレクトリ**（`~/.aish/plugins.d` または `~/.config/aish/plugins.d`）に置いた manifest からのみ有効です。プロジェクト直下の `.aish/plugins.d` は MVP では読みません。
- ヘッドレスでないブラウザ（`browser_launch` で `headless: false`）は、GUI がある環境でのみ利用してください。

## 参考

- [docs/external-tools.md](../../../docs/external-tools.md) - 外部ツールの仕様と manifest 形式
