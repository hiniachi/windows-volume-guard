# Windows Volume Guard

Windowsで新しく作られたアプリ音声セッションが100%だった場合、指定した音量まで自動的に下げる小さなRust製常駐ツールです。

> A tiny Rust utility that lowers newly-created Windows audio sessions when they start at 100%.

新しいアプリやゲームを開いた瞬間の大音量を防ぎつつ、過去に手動で50%などへ調整したアプリの設定は標準では変更しません。ネットワーク通信、テレメトリー、管理者権限は不要です。

## 動作

- 既定値は30%
- 新規セッションが99.5%以上の場合だけ30%へ下げる
- すでに30%未満なら音量を上げない
- Windowsの「システム サウンド」は標準で除外
- USB/Bluetoothを含む、すべてのアクティブな出力デバイスを監視
- Windows 10 / 11対応

Windowsは初回の音声セッションを1.0（100%、減衰なし）で生成します。本ツールはCore Audioの`IAudioSessionNotification`を使い、新規セッション作成通知を受けた時点で`ISimpleAudioVolume`を調整します。

## インストール

Rustがインストール済みなら、PowerShellで次を実行します。

```powershell
cargo install --git https://github.com/hiniachi/windows-volume-guard
windows-volume-guard install --volume 30
```

`install`は実行ファイルを`%LOCALAPPDATA%\WindowsVolumeGuard`へコピーし、現在のユーザーのサインイン時に非表示で起動するよう登録します。管理者権限は不要です。

GitHub Releasesからダウンロードした場合は、展開先で次を実行します。

```powershell
.\windows-volume-guard.exe install --volume 30
```

## 使い方

一時的に実行する場合:

```powershell
windows-volume-guard run --volume 30
```

指定できる主なオプション:

```text
--volume <0..100>     新規セッションの目標音量（既定: 30）
--cap                 100%のセッションだけでなく、目標値を超える全新規セッションを制限
--include-existing    起動時点ですでに存在するセッションも処理
--include-system      Windowsのシステム サウンドも処理
```

例として、新規・既存を問わず40%より大きいセッションを制限するには:

```powershell
windows-volume-guard install --volume 40 --cap --include-existing
```

状態確認と削除:

```powershell
windows-volume-guard status
windows-volume-guard uninstall
```

`uninstall`は自動起動を解除して常駐プロセスを停止します。インストール済み実行ファイルは、実行中ファイルの削除競合を避けるため残します。不要なら`%LOCALAPPDATA%\WindowsVolumeGuard`を手動で削除してください。

## 制限事項

- アプリがセッション作成後に自分で音量を書き換えた場合、その変更を優先します。本ツールは継続的にユーザー設定を上書きしません。
- 排他モードで音声デバイスを直接使用するアプリは、Windowsのセッションミキサーを迂回するため対象外です。
- 標準モードでは、100%へ手動設定したアプリとWindowsの初期値100%を区別できません。100%で始まる新規セッションは30%へ下がります。
- 新しく接続された出力デバイスは最大約2秒で監視対象になります。そのデバイス上で同時に始まった最初の音声は間に合わない場合があります。

## ビルドとテスト

```powershell
cargo test
cargo build --release
```

GitHub ActionsでもWindows上の`fmt`、`clippy`、テスト、リリースビルドを実行します。

## 参考資料

- [Microsoft: ISimpleAudioVolume](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-isimpleaudiovolume)
- [Microsoft: IAudioSessionNotification](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessionnotification)
- [Microsoft: IAudioSessionEnumerator](https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessionenumerator)

## License

MIT
