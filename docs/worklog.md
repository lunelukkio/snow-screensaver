# 作業ログ (worklog)

## 全体の要点
- Windows用の雪screensaver。Rust + Direct3D 11で全画面・最前面に描き、右クリックmenuで設定し、double-clickで終了する。方針はCPU・GPU・memoryの最小化で、見た目の精細さは下げてよい
- 既定設定（15fps・解像度50%）でprocess CPUは論理コア1個の0.68%、private working setは1.7 MB、exeは260 KB（2 monitorで実測）。旧Python版は2 monitorで約95〜98%、monitor 1台では約45%
- 設計の根拠・実測・却下した方式は`docs/rust-rewrite-plan.md`、構成と作業手順は`CLAUDE.md`。計測と挙動確認は`tools/measure.py`（bench / smoke）

## やりかけ・未完了
- （任意）displayの電源off中にCPUを使っていないことを実機で確認する。確認済みなのはlock中のみ
- （任意）Independent FlipをPresentMonで確認する。今は`dwm.exe`の増分が無いことからの推測

## 前回の作業（2026-11-13）
- Rust + Direct3D 11版を実装し（Issue #1）、依頼者が見た目・右クリックmenu・色選択dialog・double-click終了を確認して受け入れた
- 受け入れを受けてPython実装を削除し、README.mdとCLAUDE.mdをRust版の内容に更新した
