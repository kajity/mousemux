---
name: systemd-daemon-ops
description: "systemd 管理下で Rust デーモンを安全に動かすための unit、運用、権限、ログ設計を扱う skill。"
---

# systemd-daemon-ops

## 目的
デーモンを Linux 上で運用可能な形へ仕上げる。
この skill はアプリコード本体よりも、起動方法、権限、ログ、unit ファイル、インストールパス、hardening を扱う。

## この skill を使う場面
- systemd unit を作る
- `ExecStart` と設定ファイルパス設計を決める
- journald ログを見やすくする
- サービスの再起動方針を決める
- 最低限の hardening を加える
- 配置先を整理する

## 前提
- `Type=simple`
- デーモン自身はバックグラウンド化しない
- systemd にプロセス管理を委譲する
- root 実行
- 標準出力または標準エラーへログを出す
- ホットリロードはプロセス内監視で行う
- `grab` と `/dev/uinput` へのアクセスが必要

## 推奨 unit
```ini
[Unit]
Description=Mouse remapper with virtual mouse and keyboard routing
After=systemd-udevd.service

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/ex-g-pro-remapper --config /etc/ex-g-pro-remapper/config.yaml
Restart=on-failure
RestartSec=2
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/etc/ex-g-pro-remapper
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

## 補足方針
- 設定を `/etc/ex-g-pro-remapper/config.yaml` に置く
- 実行バイナリは `/usr/local/bin/ex-g-pro-remapper`
- ログは journald に寄せる
- systemd の再起動に依存せず、設定変更はプロセス内で反映する

## 起動前チェック
サービス起動前に、少なくとも以下を確認すること。

- 設定ファイルが存在する
- 設定が妥当
- 対象デバイスへアクセスできる
- 対象デバイスを `grab` できる
- `/dev/uinput` を開ける
- 仮想マウスを構築できる
- 仮想キーボードを構築できる

## ログ指針
### `ERROR`
- 起動できない
- デバイスを開けない
- grab できない
- 仮想デバイス生成失敗
- 設定読込失敗

### `WARN`
- 競合ルール
- 再読込失敗で旧設定維持

### `INFO`
- 起動完了
- 監視対象
- grab 成功
- 設定再読込成功

### `DEBUG`
- 詳細イベントトレース
- mouse / keyboard の振り分け判定

## hardening の注意
- `ProtectSystem=strict` を使うなら、読込対象パスを確認する
- 設定ファイル編集との整合を確認する
- 必要最小限の書き込み許可に絞る
- root 実行は前提だが、不要な権限拡大は避ける

## 運用確認コマンド
```bash
systemctl daemon-reload
systemctl enable ex-g-pro-remapper.service
systemctl start ex-g-pro-remapper.service
systemctl status ex-g-pro-remapper.service
journalctl -u ex-g-pro-remapper.service -b
```

## 出力の期待形
この skill を使ったときは、次を返す。

- unit ファイル全文
- 推奨配置先
- 起動前チェック
- 運用コマンド
- hardening の注意点
- ログ確認方法
