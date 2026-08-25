# docs/adrs — Second ADR Series (adr-017 … adr-040)

This directory holds a **second, newer ADR series** that continues numbering from
`docs/adr/` but does not share its sequence. Numbers **017, 018, and 019 collide**
with `docs/adr/ADR-017..019` while describing entirely different topics:

| Number | docs/adr/ (series 1)              | docs/adrs/ (this series)            |
| ------ | --------------------------------- | ----------------------------------- |
| 017    | Execution runtime ABI             | Runtime event stream ABI            |
| 018    | Strategy SDK                      | Capability binary interface         |
| 019    | Primitive/execution graph alignment | Capability host interface         |

When citing an ADR by number alone, cite the file path. New ADRs should continue
the **docs/adrs** series (041+) to keep the collision bounded.
