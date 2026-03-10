# AISH 外部ツールプラグイン サンプル

このディレクトリには、AISH の「外部ツールプラグイン」のサンプル実装を置いています。

## サンプル一覧

| サンプル | 説明 |
|----------|------|
| [playwright-plugin](./playwright-plugin/) | **Playwright** によるブラウザ操作（起動・遷移・クリック・入力・スクリーンショットなど） |

## 使い方

- **正式な manifest 形式は TOML**（`plugin.toml`）です。各サンプルフォルダ内の **`plugin.toml.example`** を参照し、必要に応じてコピー・編集してから配置してください。
- **探索場所**（いずれかに配置すれば読み込まれる）:
  - プロジェクト直下: `{project_root}/.aish/plugins/`（サブディレクトリに `plugin.toml` を置く、または直下に `plugin.toml`）
  - ユーザー設定: `config/plugins` または `config/plugins.d`（`AISH_HOME` または `$XDG_CONFIG_HOME/aish` の下）
  - レガシー: `~/.aish/plugins.d/`

詳細は [docs/external-tools.md](../../docs/external-tools.md) を参照してください。

### Legacy YAML について

旧形式の YAML manifest（`*.yaml`）も同じ探索場所で読み込まれます。新規には **plugin.toml（TOML）** を推奨します。YAML サンプルは `manifest.yaml.example` として「互換用」で残している場合があります。
