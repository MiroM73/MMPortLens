# Country and ASN of remote addresses (offline)

## Summary
Show where a remote address belongs: country flag / code and the network owner (ASN, e.g. "AS8075 Microsoft").

## Details
- Offline database chosen by the user (e.g. DB-IP Lite / MaxMind GeoLite2 `.mmdb` for country, `ip2asn` TSV for ASN) – nothing is sent anywhere.
- Settings: path to the database files, download link in the dialog; columns Country, ASN / Organization.
- Private / loopback / link-local addresses shown as such.
- Lookup in a worker thread with a cache.

## Acceptance
- With a database present the columns fill for public addresses; without it the columns show a hint in the tooltip.
