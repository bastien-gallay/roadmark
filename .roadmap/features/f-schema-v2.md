+++
id = "F-schema-v2"
type = "feature"
class = "differentiator"
effort = "L"
area = ["core"]
horizon = "shipped"
status = "done"
target = ["v0.2"]
shipped = { version = "v0.2.0", date = "2026-07-12", pr = 1 }
shipped_order = 6
+++

Config-owned field taxonomies: `type`/`class`/`effort`/`area`/`horizon`/`severity` values are declared per-project in `config.toml` `[fields.*]`, not hardcoded.

Closed sets, `multi` shape, `required_when` conditions with AND semantics;
horizon sort order comes from the declared value order.
