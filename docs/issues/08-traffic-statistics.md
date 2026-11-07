# Bytes sent / received per connection

## Summary
TCPView shows sent / received packets and bytes per connection. Add the same, plus rates.

## Details
- `SetPerTcpConnectionEStats` (enable `TcpConnectionEstatsData`) + `GetPerTcpConnectionEStats` per TCP connection – requires administrator rights; IPv6 variants exist (`GetPerTcp6ConnectionEStats`).
- Columns: Sent bytes, Received bytes, Sent / s, Received / s (computed from the difference between refreshes), optionally packets and RTT (`TcpConnectionEstatsPath`).
- Collection only while one of these columns is visible (it costs CPU).
- UDP has no per-endpoint counters – the ETW issue can provide them.

## Acceptance
- Downloading a large file shows a growing Received bytes value and a sensible rate.
