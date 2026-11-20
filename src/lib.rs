//! Villager In A Bucket — a Horizon native port of the Fabric mod
//! `villagerinabucket` (JustDoom/VillagerInABucket).
//!
//! Right-clicking a villager, wandering trader or zombie villager with an
//! empty bucket puts it into a `villager_in_a_bucket:*_bucket` item whose
//! `minecraft:custom_data` component carries the entity's saved NBT (as
//! `/data get entity` shows it). Right-clicking a block with that filled
//! bucket summons the villager back in front of the clicked face and swaps
//! the item for an empty bucket.
//!
//! Feasibility note: this is real gameplay, not a client visual. The
//! pickup and placement are done by the *server* half of both hooks
//! (`HostSide::Server`), so they work in single player and on a Horizon
//! server (dedicated or LAN host) that has the mod. The client half only
//! predicts the consume and arm swing; on a remote *vanilla* server there
//! is no Horizon server half, so nothing happens beyond the client-side
//! swing — a vanilla server cannot be made to accept a new item or an
//! authoritative capture by a client mod.

use mod_api::{
    Compound, EntityInteractEvent, Hand, Hooks, HostSide, Identifier, InteractionResult, Item,
    ItemKey, ItemPart, ItemStack, ItemsEvent, PlayerRef, Registrar, RegistrationContext,
    ServerHost, Tag, UseOnEvent,
};

/// The empty bucket used for pickup and returned on placement.
const BUCKET: &str = "minecraft:bucket";

/// `(entity kind, item path)` for each mob the mod can bucket; item paths
/// are bare, the item id is `villager_in_a_bucket:<path>`.
const KINDS: [(&str, &str); 3] = [
    ("minecraft:villager", "villager_bucket"),
    ("minecraft:wandering_trader", "wandering_trader_bucket"),
    ("minecraft:zombie_villager", "zombie_villager_bucket"),
];

const PICKUP_SOUND: &str = "minecraft:item.bucket.fill_axolotl";
const PLACE_SOUND: &str = "minecraft:item.bucket.empty_axolotl";

/// The full item id (`villager_in_a_bucket:<path>`) for a mob kind.
fn bucket_for_kind(kind: &str) -> Option<String> {
    KINDS
        .iter()
        .find_map(|(k, path)| (*k == kind).then_some(format!("villager_in_a_bucket:{path}")))
}

/// The stack the player's `hand` writes to: 0..=8 main hand, 40 off hand.
fn hand_slot(host: &mut dyn ServerHost, player: PlayerRef, hand: Hand) -> Option<u32> {
    match hand {
        Hand::Main => player.selected_slot(host).map(|slot| slot as u32),
        Hand::Off => Some(40),
    }
}

/// Vanilla `ItemUtils.createFilledResult`: creative keeps the used stack
/// and adds the result (when not already there); otherwise the stack is
/// replaced by the result, or shrunk by one with the result given.
fn swap_held(
    host: &mut dyn ServerHost,
    player: PlayerRef,
    hand: Hand,
    held: &ItemStack,
    result: ItemStack,
) -> bool {
    if player.has_infinite_materials(host).unwrap_or(false) {
        return player.give_stack(host, result).is_ok();
    }
    let Some(slot) = hand_slot(host, player, hand) else {
        return false;
    };
    if held.count() > 1 {
        player
            .set_inventory_slot(host, slot, held.clone().shrink(1))
            .is_ok()
            && player.give_stack(host, result).is_ok()
    } else {
        player
            .set_inventory_slot(host, slot, Some(result))
            .is_ok()
    }
}

/// The custom-data compound on a filled bucket, as written at pickup.
fn captured(stack: &ItemStack) -> Option<&Compound> {
    match stack
        .components()?
        .as_nbt()
        .get(&"minecraft:custom_data".into())
    {
        Some(Tag::Compound(data)) => Some(data),
        _ => None,
    }
}

/// The bucket-of-mob item placing its mob in front of the clicked face.
#[derive(Clone)]
struct BucketPart {
    kind: &'static str,
}

#[mod_api::horizon_part]
impl ItemPart for BucketPart {
    fn use_on(&self, host: HostSide<'_>, event: &mut UseOnEvent<'_>) -> Option<InteractionResult> {
        match host {
            HostSide::Client(_) => {
                // Predict the consume and swing for a genuinely filled bucket.
                if captured(event.stack).is_some() {
                    Some(InteractionResult::Success)
                } else {
                    Some(InteractionResult::Fail)
                }
            }
            HostSide::Server(host) => {
                let Some(data) = captured(event.stack) else {
                    return Some(InteractionResult::Fail);
                };
                let kind = match data.get(&"kind".into()) {
                    Some(Tag::String(kind)) => kind.to_string_lossy(),
                    _ => self.kind.to_owned(),
                };
                let Some(Tag::Compound(nbt)) = data.get(&"entity".into()) else {
                    return Some(InteractionResult::Fail);
                };
                let nbt = nbt.clone();

                let Some(player) = host.player(event.player) else {
                    return Some(InteractionResult::Fail);
                };
                let Some(dimension) = player.dimension(host) else {
                    return Some(InteractionResult::Fail);
                };
                // `MobBucketItem.useOn`: the mob joins in the block space in
                // front of the face that was clicked.
                let at = [
                    f64::from(event.pos[0] + event.face[0]) + 0.5,
                    f64::from(event.pos[1] + event.face[1]),
                    f64::from(event.pos[2] + event.face[2]) + 0.5,
                ];
                let Some(id) = host.summon(&dimension.to_string(), &kind, at, nbt) else {
                    return Some(InteractionResult::Fail);
                };
                if let Some(entity) = host.entity(id) {
                    entity.play_sound(host, PLACE_SOUND, 1.0, 1.0);
                }

                // Hand an empty bucket back, as `createFilledResult` does.
                let Some(bucket) =
                    Identifier::parse(BUCKET).ok().and_then(|id| host.item_key(&id))
                else {
                    return Some(InteractionResult::Fail);
                };
                if player.has_infinite_materials(host).unwrap_or(false) {
                    // preventDuplicates: no second plain bucket for creative.
                    let has_empty = player
                        .inventory(host)
                        .map(|slots| {
                            slots
                                .iter()
                                .any(|s| s.item() == bucket && s.components().is_none())
                        })
                        .unwrap_or(false);
                    if !has_empty {
                        let _ = player.give_stack(host, ItemStack::of(bucket));
                    }
                } else {
                    let Some(slot) = hand_slot(host, player, event.hand) else {
                        return Some(InteractionResult::Fail);
                    };
                    if event.stack.count() > 1 {
                        if player
                            .set_inventory_slot(host, slot, event.stack.clone().shrink(1))
                            .is_ok()
                        {
                            let _ = player.give_stack(host, ItemStack::of(bucket));
                        }
                    } else {
                        let _ =
                            player.set_inventory_slot(host, slot, Some(ItemStack::of(bucket)));
                    }
                }
                Some(InteractionResult::SuccessServer)
            }
        }
    }
}

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    /// An empty bucket right-clicked on a villager, wandering trader or
    /// zombie villager picks it up, trading UI and all — `entity_interact`
    /// runs before the entity's own `interact`, so answering here preempts
    /// trading exactly like upstream's mixin.
    fn entity_interact(
        &self,
        host: HostSide<'_>,
        event: &mut EntityInteractEvent<'_>,
    ) -> Option<InteractionResult> {
        let Some(bucket_item) = bucket_for_kind(event.kind) else {
            return None;
        };
        let Some(stack) = event.stack else {
            return None;
        };
        match host {
            HostSide::Client(host) => {
                let bucket = Identifier::parse(BUCKET)
                    .ok()
                    .and_then(|id| host.item_key(&id))?;
                (stack.item() == bucket).then_some(InteractionResult::Success)
            }
            HostSide::Server(host) => {
                let bucket = Identifier::parse(BUCKET)
                    .ok()
                    .and_then(|id| host.item_key(&id))?;
                if stack.item() != bucket {
                    return None;
                }
                let filled_key: ItemKey = Identifier::parse(&bucket_item)
                    .ok()
                    .and_then(|id| host.item_key(&id))?;
                let entity = host.entity(event.entity as u32)?;
                let player = host.players().acting_player()?;

                // The whole saved entity (`saveWithoutId`), minus what a
                // fresh summon must not inherit. VillagerData (profession,
                // level, type), Offers, custom names and the rest all ride
                // along untouched — the data preservation upstream relies on.
                let mut nbt = entity.nbt(host)?;
                for key in ["UUID", "Pos", "Rotation", "Motion", "Dimension"] {
                    nbt.remove(&key.into());
                }
                // Vanilla bucket-caught mobs do not despawn once released.
                nbt.insert("PersistenceRequired".into(), Tag::Byte(1));

                let mut filled = ItemStack::of(filled_key);
                let mut data = Compound::new();
                data.insert("kind".into(), Tag::String(event.kind.into()));
                data.insert("entity".into(), Tag::Compound(nbt));
                let mut patch = Compound::new();
                patch.insert("minecraft:custom_data".into(), Tag::Compound(data));
                if filled.apply_components(&patch).is_err() {
                    return None;
                }

                if !swap_held(host, player, event.hand, stack, filled) {
                    return Some(InteractionResult::Fail);
                }

                entity.play_sound(host, PICKUP_SOUND, 1.0, 1.0);
                entity.remove(host, mod_api::RemovalReason::Discarded);
                Some(InteractionResult::SuccessServer)
            }
        }
    }

    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for (kind, path) in KINDS {
            let item = format!("villager_in_a_bucket:{path}");
            let key = r.item_key(&item);
            r.item_part(key, BucketPart { kind });
        }
    }

    /// Declare the three filled buckets on top of a vanilla filled bucket
    /// (`tadpole_bucket` already stacks to 1 and has bucket arm motion).
    fn items(&self, _: &RegistrationContext, event: &mut ItemsEvent<'_>) {
        for (_, path) in KINDS {
            let mut declaration = Item::new(path, "minecraft:tadpole_bucket");
            declaration.max_stack_size = Some(1);
            event.items.push(declaration);
        }
    }
}
