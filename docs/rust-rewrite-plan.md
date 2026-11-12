# Rust + Direct3D 11 への書き直し計画（resource消費の最小化）

- 状態: Rust版を実装済み。依頼者の確認待ち（Python版は未削除。結果は14章）。作成日 2026-11-13
- 計画時のmodel / effort: Fable 5.2 / max。usage上限の残量はruntimeから取得できず、判定不能。
- 関連Issue: #1

## 1. 目的

常時表示する雪のscreensaverが使うCPU・GPU・memoryを、できるだけ小さくする。
見た目の精細さと滑らかさは、必要なら下げてよい（依頼者の方針）。

## 2. 現状（実測）

計測条件は全項目で共通。

- monitor 2台（4K 120 Hz + FHD 60 Hz）に全画面・最前面で表示
- 雪は1画面200個（合計400個）、Size 18、Edge blur 2、Speed 0.3
- CPUは「論理コア1個を100%」とした値（cycle単位で計測、計測区間6秒）
- 24論理コアのdesktop CPUとdiscrete GPU、Windows 11。ほかのappが動いている普段使いの状態
- `dwm.exe`（画面合成）は「静止した全画面windowを出した状態」との差で示す。ばらつきは約±0.3ポイント

現行（Python + tkinter Canvas）の結果:

| 状態 | process CPU | `dwm.exe`の増分 | private working set |
|---|---|---|---|
| 起動直後（雪の半分が画面外） | 55% | +2.2 | 17.1 MB |
| 定常状態（3分弱で到達） | 95〜98% | 未計測（同条件の対照なし） | 同上 |

原因はTk Canvasの描画方式にある。Pythonの計算は全体の約1%にすぎない。

- 変更箇所を1つの外接矩形にまとめるため、毎フレーム全画面を描き直す
- 半透明画像の合成が1個あたり約48µsかかる
- `after()`の刻みが15.6msなので、FPS設定30は実際には約22fpsになる

## 3. 方式の比較（実測）

Rustの試作を同じ条件で計測した。VRAMはprocessの専用GPU memory。

| 方式 | 設定 | process CPU | `dwm.exe`の増分 | private working set | 備考 |
|---|---|---|---|---|---|
| D3D11 flip model | 30fps・解像度100% | 1.15〜1.20% | +1.1 | 10 MB | VRAM 47 MB |
| D3D11 flip model | 15fps・100% | 0.61〜0.75% | +0.7 | 10 MB | VRAM 47 MB |
| D3D11 flip model | 15fps・50% | 0.61% | +0.7 | 10 MB | VRAM 17.6 MB |
| **同上 + 起動後のtrim** | **15fps・50%** | **0.64%** | **+0.6** | **1.6 MB** | **採用。working set 4.2 MB** |
| D3D11 flip model | 10fps・50% | 0.41% | +0.3 | 10 MB | |
| D3D11 blt model | 30fps・100% | 1.24〜1.29% | +1.8 | 10 MB | commit 95〜97 MB |
| GDI（不透明sprite） | 30fps | 4.8〜5.0% | +1.6 | 0.7 MB | `dwm.exe`のGPU負荷が約3.4ポイント増 |
| GDI（不透明sprite） | 15fps | 3.5% | 未計測 | 0.7 MB | 雪同士が重なると欠ける |
| DirectComposition | 30fps | 0.80% | 約+9〜10 | 0.9 MB | 不採用 |

読み取れること:

- D3D11 flip modelのCPU時間は1フレームあたり約0.40ms（2画面分）で、fpsに比例する。描画threadの処理（約0.24ms）の約9割は`Present`の呼び出し
- 解像度を50%にすると、processのGPU使用率が0.43%から0.12%へ、VRAMが47 MBから17.6 MBへ減る
- 起動後に1回working setをtrimすると、常駐memoryは10 MBから1.6 MBへ減る。commit（約54 MB）は減らない
- GDIはmemoryが最小だが、CPUが約6倍で、`dwm.exe`のGPU負荷も増える
- DirectCompositionは描画を`dwm.exe`へ移すだけで、総負荷は最大になった
- 高精度timerにより、設定fpsと実測fpsが一致した（30.00 / 15.00 / 10.00）

試して効果を確認できなかったもの（採用しない）: EcoQoS（効率mode）、`WS_EX_NOREDIRECTIONBITMAP`。

## 4. 採用する設計

言語はRust、描画はDirect3D 11。右クリックmenuなどの機能はWin32 APIで作る。

- **window**: monitorごとに枠なし・最前面の全画面window。cursorは非表示
- **描画**: monitorごとにflip modelのswap chain（`DXGI_SWAP_EFFECT_FLIP_DISCARD`、`DXGI_SCALING_STRETCH`）。deviceは1つを共有
- **雪**: 1回のinstanced drawで全部を描く。円とぼかしはpixel shaderで計算するので、画像の生成が要らない
- **解像度**: back bufferをmonitorの100 / 50 / 33 / 25%にできる。拡大は合成側に任せる。既定は50%
- **速度の計算**: 現行と同じ式（落下は`vy * dt * 60`、横揺れは`sin`）。小数座標で描くので動きは滑らかになる
- **frame間隔**: 高精度waitable timer（`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`）。使えない環境では通常のtimerへ切り替える
- **FPS**: 既定を15にする。menuの選択肢に5を足す（5〜120）
- **見えていない間は止める**: displayの電源off、session lockの間は描画とtimerを止める
- **memory**: 起動直後とmenu操作の後に1回ずつworking setをtrimする
- **復旧**: device lostとdisplay構成の変更では、起動時と同じ手順で作り直す
- **設定file**: 現行と同じ`config.json`（exeと同じfolder）。既存のkeyはそのまま読む。追加keyは`fps`（既定15）と`resolution`（既定50）
- **依存crate**: `windows`（Microsoft公式binding）、`serde` / `serde_json`（設定file）、build時のみ`winresource`（icon埋め込み）
- **shader**: `fxc`でcompileした結果をrepositoryに置く。build時にWindows SDKを要求しない

現行から変わる点:

- 輪郭のぼけ方の式が変わる（Gaussian blurからsmoothstepへ）。見た目はほぼ同じ
- 既定の解像度50%では、雪がわずかに柔らかく見える
- menuや色選択dialogを開いている間は、雪が止まる
- Alt+F4などのclose要求でも終了する

## 5. 範囲と除外

範囲:

- Rustでの再実装（現行機能の維持と、4章の追加機能）
- unit test（設定fileの読み書き、雪の位置計算）
- 計測tool（`tools/measure.py`）とbuild script（`build.ps1`）
- README.mdとCLAUDE.mdの更新
- 動作確認後のPython実装の削除

除外:

- `.scr`形式でのWindowsへの登録、自動起動
- 多重起動の防止
- monitor選択のmenu化（現行どおり`config.json`で指定）
- HDR出力、code signing

## 6. 変更するfile

| 区分 | file |
|---|---|
| 追加 | `Cargo.toml`、`Cargo.lock`、`build.rs`、`build.ps1` |
| 追加 | `src/main.rs`、`src/config.rs`、`src/flakes.rs`、`src/render.rs`、`src/menu.rs` |
| 追加 | `shaders/snow.hlsl`、`shaders/snow_vs.cso`、`shaders/snow_ps.cso` |
| 追加 | `tools/measure.py`、`docs/rust-rewrite-plan.md` |
| 変更 | `README.md`、`CLAUDE.md`、`.gitignore`（`target/`を追加） |
| 削除（受け入れ後） | `main.py`、`snow/`、`installer/build_exe.py`、`pyproject.toml`、`uv.lock`、`.python-version` |
| 維持 | `installer/snow.ico`、`installer/make_icon.py` |

Git管理外のfile:

- `dist/snow.exe`は新しいbuildで上書きする（現行のPyInstaller版はGit履歴から再生成できる）
- `dist/config.json`が無ければ、rootの`config.json`をcopyして設定を引き継ぐ
- `.venv/`、`build/`、`__pycache__/`は不要になる。削除は別途確認してから行う

## 7. 手順

1. Issueを登録し、作業branch `issue-<番号>-rust-rewrite` を作る
2. crateの骨組み、`config.rs`、`flakes.rs`をunit test付きで作る
3. `render.rs`とwindow、frame loopを作り、画面写真で表示を確認する
4. menu、色選択、設定の保存を作る
5. 描画停止、作り直し、trimを入れる
6. `build.ps1`とiconを入れ、`tools/measure.py`で実測して8章と照合する
7. commit 1（Rustを追加。Pythonはまだ残す）。ここで依頼者が見た目と操作を確認する
8. 受け入れ後にcommit 2（Pythonを削除、文書を更新）。mainへfast-forwardする
9. pushは依頼者が行う（public repositoryのため）

## 8. 完了条件

AIが確認する:

- `cargo build --release`、`cargo test`、`cargo clippy -- -D warnings`、`cargo fmt --check`が通る
- 既定設定（15fps・解像度50%）で、process CPUが論理コア1個の1.0%以下
- 同条件で、`dwm.exe`の増分が+1.5ポイント以下
- 定常状態のprivate working setが5 MB以下、working setが10 MB以下
- FPS設定10 / 15 / 30で、実測fpsとの差が±3%以内
- 画面写真で両方のmonitorに雪が出ており、既存の`config.json`の数・大きさ・色が反映されている
- menu項目に相当するmessageを送ると各設定が変わり、`Save settings`で`config.json`に書かれる
- double-clickに相当するmessageで終了する
- session lockに相当するmessageでCPUがほぼ0になり、unlockで再開する
- display構成の変更に相当するmessageで、作り直しが成功する
- `snow.exe`が1 MB以下（現行は18.1 MB）

依頼者が確認する:

- 既定設定の見た目と動きに問題がない（合わなければmenuで調整）
- 右クリックmenuと色選択dialogが前面に出る
- 手でdouble-clickして終了できる
- displayの電源が切れている間、CPUを使っていない（任意）

## 9. 検証command

```powershell
cargo test
cargo clippy -- -D warnings
cargo fmt --check
./build.ps1
uv run --no-project python tools/measure.py dist/snow.exe
```

`tools/measure.py`は対象を起動し、process CPU、`dwm.exe`のCPU、processごとのGPU使用率とGPU memory、working setを計測して終了させる。

## 10. 統合と承認範囲

この計画の承認に含めるもの:

- Issueの登録と更新（public repository。個人情報とlocal pathは書かない）
- 作業branchの作成、6章のfileの追加・変更、local commit
- crateのdownload（crates.io）とbuild、`dist/snow.exe`の上書き

承認に含めないもの（その時点で確認する）:

- Python実装の削除とmainへの統合（手順7の受け入れ後）
- `.venv/`などの不要folderの削除
- push（依頼者が実行する。commandは提示する）
- Issueのclose

## 11. riskと未確認事項

- **Independent Flipの成否**: 成立すれば`dwm.exe`の増分はほぼ無くなる。試作では成立していなかったとみられる。実装後の結果は14章。直接の確認にはPresentMonが要る
- **1台での計測**: 数値はこのPCとdriverでのもの。driverが確保するmemory量は環境で変わる
- **GPUの消費電力**: ばらつきが大きく、方式間の差は判定できなかった
- **device lost**: 実際のdriver resetでは試せない。display構成変更のmessageで同じ経路を通して確認する
- **trimの限界**: 常駐memoryは減るが、commit（約54 MB）は減らない。trim直後の1フレームだけ約40ms遅れる
- **見た目**: 4章の「現行から変わる点」は、依頼者の確認で合否を決める
- **Tkとの差**: 現行もmenuを開いている間に雪が止まるかは未確認

## 12. 出典

- Microsoft Learn「For best performance, use DXGI flip model」（flip modelは性能と消費電力で有利。swap chainの解像度を下げる選択肢。DirectFlip / Independent Flipの条件）
  <https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model>
- Microsoft Learn `CreateWaitableTimerExW`（`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`はWindows 10 version 1803以降）
  <https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createwaitabletimerexw>
- Microsoft Learn `SetProcessWorkingSetSize`（両方に`(SIZE_T)-1`を渡すとworking setから可能な限りpageを外す）
  <https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-setprocessworkingsetsize>
- crates.io `windows` 0.62.2（必要なfeatureがあることを確認）
  <https://crates.io/crates/windows>
- Windows SDK 10.0.19041.0 のheader（`d3d11.h`、`dxgi1_2.h`、`dcomp.h`、`winnt.h`、`WinUser.h`、`WtsApi32.h`）で、使うAPIと定数を確認
- 実測: 2章と3章。試作と計測scriptは一時folderにあり、計測方法は`tools/measure.py`として残す

## 13. 規模と相場の評価

- **判定**: 適正。約450行のPythonを約700〜800行のRustで置き換える。個人projectとしては大きめだが、CPUは実測で約1/150になり、依頼（最小のresource、Rust可）に対して最小の構成
- **相場**: 常時動く2D animationは、compile言語とGPU描画（Direct3D、OpenGL、SDL2）で作るのが一般的。RustからWin32とDirect3Dを使う場合は`windows` crateが標準
- **簡素化**: Pythonのまま描画だけDirect2Dへ差し替える案（ctypes約200行）。実測でCPU約2.3%、memory約68 MB。変更は小さいが、CPU・memory・起動時間のすべてでRust案に劣る
- **代替**: `winit` + `wgpu`は記述が楽だが、依存と常駐memoryが大きい（未計測）。`.scr`として登録すれば無操作時だけ自動で起動できるが、今回の範囲外

## 14. 実装後の実測（2026-11-13）

`dist/snow.exe`（260 KB）を`tools/measure.py`で計測した。条件は2章と同じ。

| 設定 | 実測fps | process CPU | `dwm.exe`の増分 | processのGPU使用率 | working set | private working set | VRAM |
|---|---|---|---|---|---|---|---|
| 15fps・50%（既定） | 15.00 | 0.68% | +0.3 | 0.33% | 4.3 MB | 1.7 MB | 35 MB |
| 10fps・50% | 10.00 | 0.49% | -0.3 | 0.26% | 4.2 MB | 1.7 MB | 35 MB |
| 30fps・50% | 30.00 | 1.25% | +0.4 | 0.68% | 4.6 MB | 1.9 MB | 35 MB |
| 15fps・100% | 15.00 | 0.69% | -0.1 | 1.91% | 4.4 MB | 1.8 MB | 94 MB |

- `bench`の8項目、`smoke`の14項目がすべて合格した。`cargo test`（19件）、`cargo clippy -- -D warnings`、`cargo fmt --check`も通る
- session lock中（描画停止中）のCPUは0.02〜0.03%で、動作中の約1/30。driverのthreadとwindow messageの分が残る
- commitは約56 MBで、試作と同じ

試作の値（3章）との違い:

- `dwm.exe`の増分がfpsに比例しなくなり、ばらつき（約±0.3）の範囲に収まった。Independent Flipが成立している可能性が高い。PresentMonでの直接確認はしていない
- そのぶん表示の処理がprocess側に計上され、processのGPU使用率（0.33%）とVRAM（35 MB）は試作より大きい。`dwm.exe`のGPU使用率は約1%から0.05%へ減った
- 解像度100%ではGPU使用率が約6倍になる。既定の50%は妥当

残っている確認は8章の「依頼者が確認する」項目。
