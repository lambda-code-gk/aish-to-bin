---
name: check-responsibility
description: Verifies that a source file's code does not violate the one-line responsibility stated at the top of the file. Use when editing a file that has a "責務:" line at the top, when the user asks to check for responsibility violations, or after changing policy/adapter/usecase code.
---

# 責務違反の確認（check-responsibility）

ファイル冒頭の「責務」一行に照らし、そのファイル内のコードが責務に反していないかを確認する。

## 手順

1. **責務の一行を特定する**  
   対象ファイルの先頭（Rust なら `//! 責務: …` のモジュール doc）に、そのモジュールの責務が一行で書かれている。それを読み、「何のみを行うか」「何をしないか」を把握する。

2. **コードと照合する**  
   ファイル内のコード（関数・分岐・参照・文字列リテラルなど）が、その責務と矛盾していないか確認する。
   - 例: 責務に「個別ツールの表示仕様は知らない」とあるのに、特定のツール名（例: `"replace_file"`）で分岐して表示用の文字列を組み立てている → **違反**。要約は port やツール側に委譲すべき。

3. **違反があれば指摘する**  
   違反している箇所を具体的に示し、責務に沿う修正方針（port の追加・委譲・別モジュールへの移動など）を提案する。

## 例（policy とツール）

- **責務**: 「ツール呼び出しの Allow/Deny/RequireApproval を決めるのみ。個別ツールの表示仕様は知らない。」
- **違反**: `if tool_name == "replace_file" { ... }` のようにツール名で分岐し、そのツール用の要約文字列を組み立てている。→ 要約は port（例: ApprovalSummaryProvider）や Tool 側に委譲する。

## トリガー

- 冒頭に `責務:` を含むファイルを編集した直後
- ユーザーが「責務違反がないか確認して」と言ったとき
- policy / usecase / adapter の境界付近のコードを変更したとき
