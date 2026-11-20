# Villager In A Bucket (Horizon native port)

A port of the Fabric mod [`villagerinabucket`](https://github.com/JustDoom/VillagerInABucket)
to [Horizon](https://github.com/horizon-rust/horizon)'s native Rust mod API.

- Right-click a **villager**, **wandering trader** or **zombie villager** with an empty
  bucket to pick it up into a `villager_in_a_bucket:*_bucket` item.
- Right-click a block face with the filled bucket to place the mob back down; the bucket
  is returned empty.

## How it works

- `Hooks::entity_interact` (both sides) runs where `Player.interactOn` starts, before the
  entity's own `interact` — answering there preempts the trading UI exactly like upstream's
  mixin, which is also what requires a plain `minecraft:bucket` in hand.
- On the server the villager's complete saved NBT (`saveWithoutId`, i.e. what
  `/data get entity` prints — `VillagerData` with profession/level/type, `Offers`,
  `CustomName`, etc.) is stored in the filled bucket's `minecraft:custom_data` component
  after stripping `UUID`/`Pos`/`Rotation`/`Motion`/`Dimension` and setting
  `PersistenceRequired` (as vanilla-bucketed mobs get). The entity is then discarded.
- `ItemPart::use_on` on the three mod items re-summons the stored mob in front of the
  clicked face (`host.summon`, the `/summon <kind> <pos> <nbt>` path) and hands an empty
  bucket back, following vanilla `ItemUtils.createFilledResult` semantics: in creative the
  used bucket is kept and the result is only added if the inventory has no plain bucket
  already; in survival a multi-bucket stack shrinks by one with the result added.
- The `items` registration hook declares the three items on the `minecraft:tadpole_bucket`
  template (stack size 1); `en_us.json` names them.

## Feasibility / caveats

- **Requires a Horizon host.** The capture and the spawn are authoritative server writes;
  they run in single player and on a Horizon server (integrated LAN host or dedicated) that
  loads this mod. A *client-only* install connected to a vanilla server cannot pick anything
  up — a vanilla server has no `villager_in_a_bucket` item and no capture code, and a client
  mod cannot change that. On such a server the client half merely predicts a swing.
- No sneak requirement, no cooldown — matching the upstream feature description (upstream
  exposes a config only in some versions; none is needed here).
- Cosmetic gaps versus upstream: the filled buckets reuse the tadpole bucket model (no
  per-region villager textures, no lore tooltip with the stored mob's name/profession, no
  dispenser pickup behavior, no "zucket" advancement).
- Right-clicking a block that itself consumes the click (e.g. a chest) keeps vanilla
  precedence and does not place the mob, as with vanilla bucket items.
