# package_e2e fixtures

package 解決の統合テスト用 fixture。1 fixture 1 意図。

| ディレクトリ | 意図 |
|-------------|------|
| `basic_package_task/` | project に task.d なし。package alpha の `tasks/build` のみ。package task が解決されることを検証。 |
| `package_skill/` | package alpha に task `run`（skills = ["review"]）と skill `review`。package skill が読み込まれることを検証。 |
| `package_hook/` | package alpha に system_hook と task build。project hook + package hook + task の合成順を検証。 |
| `project_overrides_package/` | project の task.d と package alpha の両方に `build`。catalog Tasks が先なので project が採用されることを検証。 |

すべての prompt/hook/task には由来が分かる識別トークン（例: `[TASK build from package-alpha]`）を入れている。
