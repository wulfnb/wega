# Dependency graph after the architecture-first refactor (Phases A-C, first round)

Generated at commit `7bffc42`.

```mermaid
graph TD
  wega_adapter_skia["wega-adapter-skia"]
  wega_cookies["wega-cookies"]
  wega_engine["wega-engine"]
  wega_network["wega-network"]
  wega_permissions["wega-permissions"]
  wega_platform["wega-platform"]
  wega_protocol["wega-protocol"]
  wega_security["wega-security"]
  wega_storage["wega-storage"]
  wega_adapter_skia --> wega_platform
  wega_engine --> wega_adapter_skia
  wega_engine --> wega_network
  wega_engine -.->|build| wega_network
  wega_engine --> wega_permissions
  wega_engine --> wega_platform
  wega_engine --> wega_protocol
  wega_engine --> wega_security
  wega_network --> wega_cookies
  wega_network --> wega_platform
  wega_network --> wega_protocol
  wega_network --> wega_storage
  wega_permissions --> wega_network
  wega_permissions --> wega_protocol
```
