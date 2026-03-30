# mousemux 設計メモ

## 1. 設計方針

初期実装は、単一マウス入力を `grab` して監視し、YAML 設定に基づいてイベントを以下の 2 系統へ振り分ける常駐デーモンとする。

- リマップ対象のボタンイベントは仮想キーボードからキーイベントを送出する
- 未リマップの移動・ホイール・通常ボタンは仮想マウスへ流す

実装上の複雑性を抑えるため、初期スコープでは以下を前提とする。

- 入力対象は単一デバイス
- キーボード変換対象はボタンイベントのみ
- 移動イベントとホイールイベントは初期実装ではパススルーのみ
- 設定フォーマットは YAML
- systemd 管理下で root 実行
- 設定変更はホットリロード対応

## 2. 推奨モジュール分割

### 2.1 `config`

責務:

- YAML 読み込み
- 構文検証
- 論理検証
- 競合ルール検出

主要処理:

- `load(path) -> Config`
- `validate(config) -> ValidationResult`
- `detect_conflicts(config) -> Vec<ConflictWarning>`

### 2.2 `device`

責務:

- 物理マウスデバイスのオープン
- `grab` / `ungrab`
- `evdev` イベント受信
- デバイス capability 取得
- 入力イベント正規化

主要処理:

- `open_mouse(path) -> Device`
- `grab(device) -> Result<()>`
- `next_event() -> NormalizedMouseEvent`
- `read_capabilities() -> SourceMouseCapabilities`

### 2.3 `virtual_mouse`

責務:

- `uinput` 仮想マウス生成
- 未リマップイベントの送出

主要処理:

- `build_from_capabilities(caps) -> VirtualMouse`
- `emit_mouse(event)`

### 2.4 `virtual_keyboard`

責務:

- `uinput` 仮想キーボード生成
- キー送出

主要処理:

- `build(keys) -> VirtualKeyboard`
- `emit(sequence)`

### 2.5 `router`

責務:

- 入力イベントとルールの照合
- マウスへ流すか、キーボードへ変換するかの判定

主要処理:

- `route(event, rules) -> RoutedAction`

補足:

- 競合ルールはロード時に解決方針を確定する
- 実行時は「最後に有効となるルール」のみ参照すると単純化できる

### 2.6 `reload`

責務:

- 設定ファイル変更監視
- 新設定の読込と差し替え
- 失敗時のロールバック

主要処理:

- `watch(path)`
- `reload_if_changed()`

## 3. 推奨ランタイム構成

単一プロセス内で以下のタスクを持つ構成を推奨する。

1. 入力イベント受信タスク
2. 設定ファイル監視タスク
3. 終了シグナル監視タスク

共有状態:

- 現在有効な設定
- 現在有効なルール集合
- 仮想マウスハンドル
- 仮想キーボードハンドル
- 現在監視中の物理デバイス情報

共有状態の更新は `Arc<RwLock<...>>` または同等の読多書少構造を想定する。

## 4. ホットリロード方針

### 4.1 基本方針

- ファイル変更検知時に設定を再読込する
- 新設定の構文検証と論理検証が通った場合のみ有効化する
- 検証失敗時は旧設定を維持する

### 4.2 差し替え単位

最低限、以下を再構築対象とする。

- ルール集合
- 使用キー一覧
- 監視デバイス情報

仮想デバイスについては以下の方針を推奨する。

- 仮想キーボードは使用キーが変わる場合に再生成する
- 仮想マウスは監視デバイスが変わるか、元デバイス capability が変わる場合に再生成する

推奨:

- 設定変更適用前に新しい状態を丸ごと組み立てる
- 差し替えは一括で行い、中途半端な状態を公開しない

## 5. 競合ルール処理

競合の定義:

- 同一の入力条件に対して複数ルールが存在する状態

解決方針:

- 設定ファイルで後に書かれたルールを採用する
- 先に書かれたルールは無効化扱いとする
- ロード時に警告ログを出力する

警告ログに含めるべき情報:

- 入力条件
- 優先されたルールの位置
- 無効化されたルールの位置

## 6. ルーティング方針

### 6.1 基本方針

- 物理マウスからのイベントはすべて一度プロセスで受ける
- `grab` により元デバイスからのイベントは OS に直接流さない
- リマップ対象のボタンイベントは仮想キーボードへ送る
- 未リマップのボタンイベント、移動イベント、ホイールイベントは仮想マウスへ送る

### 6.2 初期実装での対象

- remap 対象: ボタン押下・解放
- passthrough 対象: 移動、ホイール、未リマップボタン
- 非対象: 複雑なジェスチャ、複数デバイス統合、マクロ DSL

## 7. ログ方針

ログレベル案:

- `ERROR`: 起動失敗、設定読込失敗、デバイスオープン失敗、grab 失敗、uinput 生成失敗
- `WARN`: 競合ルール、ホットリロード失敗時の旧設定維持
- `INFO`: 起動完了、対象デバイス、grab 成功、設定再読込成功
- `DEBUG`: 詳細イベントトレース、ルーティング判定

## 8. YAML スキーマ案

```yaml
device:
  path: /dev/input/by-id/usb-Example_Mouse-event-mouse

reload:
  enabled: true
  debounce_ms: 250

remaps:
  - description: right button down -> left meta down
    input:
      type: key
      code: BTN_RIGHT
      value: 1
    output:
      - key: KEY_LEFTMETA
        value: 1

  - description: right button up -> left meta up
    input:
      type: key
      code: BTN_RIGHT
      value: 0
    output:
      - key: KEY_LEFTMETA
        value: 0
```

## 9. systemd unit 案

```ini
[Unit]
Description=Mouse remapper with virtual mouse and keyboard routing
After=systemd-udevd.service

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/mousemux --config /etc/mousemux/config.yaml
Restart=on-failure
RestartSec=2
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/etc/mousemux
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

補足:

- `ProtectSystem=strict` を使う場合、読込対象パスの調整が必要
- 設定ファイルを `/etc` に置くなら `ReadOnlyPaths` または `BindReadOnlyPaths` の調整も検討対象

## 10. 実装順序案

1. YAML 読込と設定バリデーション
2. 単一マウス入力のオープンと `grab`
3. 元デバイス capability 取得と仮想マウス生成
4. 仮想キーボード生成
5. 単純な 1:1 リマップ
6. 未リマップイベントのパススルー
7. 複数キー出力
8. 競合検出と警告ログ
9. ホットリロード
10. systemd unit と運用確認
