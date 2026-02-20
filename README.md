# 社交地图 + 实时通讯 + 高性能网页游戏

## 已完成能力

- 登录/注册：JWT 鉴权
- 社交地图：MapLibre 实时点位刷新
- 实时通讯：WebSocket + MessagePack
- 聊天：发送消息 + 历史消息加载
- 聊天状态：房间成员在线状态 + 未读计数 + 已读标记
- 邀请：在线用户发起对战邀请 + 接受/拒绝状态流转

## 前端入口

- 主页联调面板：`apps/platform/src/app.rs`
- 地图控制与点位图层：`apps/platform/src/map.rs`

## 核心 API

- `POST /api/register`
- `POST /api/login`
- `POST /api/position`
- `POST /api/chat/send`
- `GET /api/chat/history?room_id=global`
- `GET /api/chat/room-state?token=...&room_id=global`
- `POST /api/chat/mark-read`
- `POST /api/invite/send`
- `GET /api/invite/pending?token=...`
- `POST /api/invite/respond`
- `GET /ws?token=...`

## 启动

```powershell
docker compose up -d
cargo leptos watch
```
