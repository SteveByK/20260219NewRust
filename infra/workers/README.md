# Cloudflare Workers (Edge)

用于边缘逻辑：
- 地理位置路由（按区域分流）
- 请求过滤（IP/UA/速率）

建议把低延迟、无状态逻辑放在 Worker，核心状态仍由 Axum 服务处理。
