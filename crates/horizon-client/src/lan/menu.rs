//! Container menus for guests (AbstractContainerMenu.doClick): the player's
//! inventory menu (window 0) and block containers (chests, barrels, hoppers,
//! dispensers, shulker boxes). The click modes are pickup, quick move, swap,
//! clone, throw, quick craft (drag) and pickup all; items thrown out of a
//! menu are returned for the game to drop.
use super::guest::Guest;
use crate::{
    bundle::{Contents, can_be_in_bundle, toggle_selected},
    containers::BlockEntity,
    items::{ItemStack, Items},
    world::World,
};

/// What the menus need to know about items.
pub trait Rules {
    fn items(&self) -> &Items;
    fn max_stack(&self, item: &str) -> u32;
    /// The armor slot (0 head .. 3 feet) an item can be worn in.
    fn armor_index(&self, item: &str) -> Option<usize>;
}

/// An open block container.
pub struct Menu {
    pub window: i32,
    /// The `minecraft:menu` registry id.
    pub kind: i32,
    pub pos: Option<(i32, i32, i32)>,
    /// Where the container's slots live.
    pub slots: Slots,
    /// Slots of the container itself (without the 36 inventory slots).
    pub size: usize,
    pub state: i32,
    /// What the client was last told, for change detection.
    pub sent: Vec<Option<ItemStack>>,
    /// An unfinished drag: (mode, slots).
    pub drag: Option<(u8, Vec<usize>)>,
    /// The data slots the client was last sent (ContainerSetData), a furnace's
    /// progress; None until sent, so any first value goes out.
    pub sent_data: Vec<Option<i16>>,
    /// Container slots the menu does not have: a mod's screen hides them, so
    /// no click or quick move reaches them (`Slot` positions are the screen's).
    pub hidden: Vec<usize>,
    /// A mod menu type's take-only slots (`FurnaceResultSlot.mayPlace`).
    pub results: Vec<usize>,
    /// The armor and off-hand slots at the end of `size` (a mod menu's `armor`): 0, 4 or 5.
    pub equip: usize,
    /// The slots of a mod menu type that take only some items.
    pub accepts: Vec<(usize, Vec<String>)>,
    /// A mod menu type's data slot count (`addDataSlots`); None for vanilla's.
    pub data_slots: Option<usize>,
}

/// What a guest did with a native mod's menu (`Slots::Native`), for the game
/// to tell the mod once the hub is back in its hands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeMenuEvent {
    /// A click (`ServerboundContainerClickPacket`): the menu's slot (its own
    /// first, then the player's 36; -999 outside), the button and the mode.
    Click {
        uuid: u128,
        window: i32,
        slot: i32,
        button: i32,
        mode: i32,
    },
    /// The menu closed: by the player, by the server, another opened, or
    /// the player left.
    Closed { uuid: u128, window: i32 },
}

/// The y of the key a chest minecart's or chest boat's items are kept under
/// while a menu shows them.
pub const VEHICLE_KEY_Y: i32 = -100_000;

/// Where the slots of a container menu live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slots {
    /// One block entity.
    Block((i32, i32, i32)),
    /// A double chest (`CompoundContainer`): the first half's slots, then
    /// the second's.
    Double((i32, i32, i32), (i32, i32, i32)),
    /// The player's own ender chest inventory (`PlayerEnderChestContainer`).
    Ender,
    /// A native mod's menu (`Guest::native_menu`): items to look at, which no
    /// click moves; the clicks go to the mod.
    Native,
}

impl Slots {
    fn halves(self) -> Vec<(i32, i32, i32)> {
        match self {
            Slots::Block(k) => vec![k],
            Slots::Double(a, b) => vec![a, b],
            Slots::Ender | Slots::Native => Vec::new(),
        }
    }
    /// Whether the block entities behind the slots are still there
    /// (`Container.stillValid`).
    pub fn valid(self, world: &World) -> bool {
        self.halves()
            .iter()
            .all(|k| world.block_entities.contains_key(k))
    }
    /// `Container.stillValid` for the guest: the block entities are there
    /// and the guest is within reach of each (`stillValidBlockEntity`). The
    /// ender chest's block is `at` (`PlayerEnderChestContainer.activeChest`).
    pub fn still_valid(self, world: &World, guest: &Guest, at: Option<(i32, i32, i32)>) -> bool {
        let blocks = match self {
            Slots::Ender => at.into_iter().collect(),
            Slots::Native => Vec::new(),
            _ => self.halves(),
        };
        // A vehicle's container is kept under a key, not a block: the game
        // checks its reach to the vehicle (`isChestVehicleStillValid`).
        self.valid(world)
            && blocks
                .iter()
                .all(|k| k.1 == VEHICLE_KEY_Y || guest.within_block_range([k.0, k.1, k.2], 4.0))
    }
    /// Container slot `n`.
    pub fn get(self, world: &World, guest: &Guest, mut n: usize) -> Option<ItemStack> {
        if self == Slots::Ender {
            return guest.ender_items.get(n).cloned().flatten();
        }
        if self == Slots::Native {
            return guest.native_menu.get(n).cloned().flatten();
        }
        for k in self.halves() {
            let slots = shown(world, k)?;
            if n < slots.len() {
                return slots[n].clone();
            }
            n -= slots.len();
            if matches!(self, Slots::Block(_)) && n < equip_of(world, k) {
                return worn_slot(guest, n).cloned().flatten();
            }
        }
        None
    }
    /// Every container slot, in menu order.
    pub fn read(self, world: &World, guest: &Guest) -> Option<Vec<Option<ItemStack>>> {
        if self == Slots::Ender {
            return Some(guest.ender_items.clone());
        }
        if self == Slots::Native {
            return Some(guest.native_menu.clone());
        }
        let mut items = Vec::new();
        for k in self.halves() {
            items.extend_from_slice(shown(world, k)?);
            // A mod menu's armor and off-hand slots are the guest's own.
            for n in 0..equip_of(world, k) {
                items.push(worn_slot(guest, n).cloned().flatten());
            }
        }
        Some(items)
    }
    /// Store what `read` gave, changed.
    pub fn write(self, world: &mut World, guest: &mut Guest, items: Vec<Option<ItemStack>>) {
        if self == Slots::Ender {
            guest.ender_items = items;
            return;
        }
        if self == Slots::Native {
            // Only the mod changes what it shows.
            return;
        }
        let mut rest = items.into_iter();
        for k in self.halves() {
            let n = shown(world, k).map_or(0, <[_]>::len);
            let equip = equip_of(world, k);
            if let Some(e) = world.block_entities.get_mut(&k) {
                for slot in e.slots_mut().iter_mut().take(n) {
                    *slot = rest.next().flatten();
                }
            }
            for worn in 0..equip {
                if let (Some(slot), Some(item)) = (worn_slot_mut(guest, worn), rest.next()) {
                    *slot = item;
                }
            }
        }
    }
}

/// How many armor and off-hand slots the menu of the mod block at `k` adds to its container (`armor` in its menu type:
/// 0, or 4 armor slots and the off-hand's).
pub fn equip_of(world: &World, k: (i32, i32, i32)) -> usize {
    let Some(e) = world.block_entities.get(&k) else {
        return 0;
    };
    if !matches!(e, BlockEntity::Mod(_)) {
        return 0;
    }
    let state = world.block(k.0, k.1, k.2).0;
    world
        .block_entities
        .mod_container(state)
        .and_then(|c| c.menu.as_deref())
        .and_then(crate::containers::mod_menu_type)
        .map_or(0, |m| m.armor)
}

/// The guest's worn slot `n`: 0 head to 3 feet, then the off-hand.
fn worn_slot(guest: &Guest, n: usize) -> Option<&Option<ItemStack>> {
    if n < 4 { guest.armor.get(n) } else if n == 4 { Some(&guest.offhand) } else { None }
}

fn worn_slot_mut(guest: &mut Guest, n: usize) -> Option<&mut Option<ItemStack>> {
    if n < 4 { guest.armor.get_mut(n) } else if n == 4 { Some(&mut guest.offhand) } else { None }
}

/// The slots of the block entity at `k` its menu shows: a mod container's
/// count, though items past it stay saved.
fn shown(world: &World, k: (i32, i32, i32)) -> Option<&[Option<ItemStack>]> {
    let e = world.block_entities.get(&k)?;
    let slots = e.slots();
    if !matches!(e, BlockEntity::Mod(_)) {
        return Some(slots);
    }
    let state = world.block(k.0, k.1, k.2).0;
    let count = world
        .block_entities
        .mod_container(state)
        .map_or(0, |c| c.slots);
    Some(&slots[..count.min(slots.len())])
}

/// Slots of a menu seen as one list.
pub struct View<'a> {
    pub guest: &'a mut Guest,
    /// The container's slots, or None for the inventory menu.
    pub container: Option<&'a mut [Option<ItemStack>]>,
    /// The container is a shulker box (`ShulkerBoxSlot`).
    pub shulker: bool,
    /// Container slots that do not exist in this menu (see [`Menu::hidden`]).
    pub hidden: &'a [usize],
    /// Container slots nothing is put into (see [`Menu::results`]).
    pub results: &'a [usize],
    /// How many of the container's last slots are the guest's own armor and off-hand (a mod menu's `armor` slots).
    pub equip: usize,
    /// Container slots that take only some items (see [`Menu::accepts`]).
    pub accepts: &'a [(usize, Vec<String>)],
}

impl View<'_> {
    pub fn len(&self) -> usize {
        match &self.container {
            Some(c) => c.len() + 36,
            None => 46,
        }
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn get(&self, i: usize) -> Option<ItemStack> {
        match &self.container {
            Some(c) if i < c.len() => c[i].clone(),
            Some(c) => self
                .guest
                .inventory
                .get(player_slot(i - c.len()))
                .cloned()
                .flatten(),
            None => self.guest.menu_slot(i),
        }
    }
    pub fn set(&mut self, i: usize, item: Option<ItemStack>) {
        match &mut self.container {
            Some(c) if i < c.len() => c[i] = item,
            Some(c) => {
                let n = c.len();
                self.guest.inventory[player_slot(i - n)] = item;
            }
            None => self.guest.set_menu_slot(i, item),
        }
    }
    /// Whether the slot takes part in clicks at all.
    fn enabled(&self, i: usize) -> bool {
        (self.container.is_some() || i >= 5) && !self.hidden.contains(&i)
    }
    /// The wearing slot the container slot `i` is, if it is one of the guest's armor slots (0 head to 3 feet, 4 the off-hand).
    fn worn(&self, i: usize) -> Option<usize> {
        let c = self.container.as_ref()?;
        let first = c.len().checked_sub(self.equip)?;
        (self.equip > 0 && i >= first && i < c.len()).then(|| i - first)
    }
    fn may_place(&self, i: usize, stack: &ItemStack, rules: &dyn Rules) -> bool {
        if let Some(k) = self.worn(i) {
            // ArmorSlot.mayPlace: the right piece for the slot; the off-hand takes anything.
            return if k < 4 { rules.armor_index(&stack.item) == Some(k) } else { true };
        }
        if self.container.is_none() && (5..=8).contains(&i) {
            return rules.armor_index(&stack.item) == Some(i - 5);
        }
        if self.shulker
            && self.container.as_ref().is_some_and(|c| i < c.len())
            && !crate::menu::fits_inside_container_items(stack)
        {
            return false;
        }
        if self.container.as_ref().is_some_and(|c| i < c.len())
            && self.accepts.iter().any(|(slot, items)| *slot == i && !items.iter().any(|i| i.as_str() == &*stack.item))
        {
            return false;
        }
        self.enabled(i) && !self.result(i)
    }
    /// A take-only container slot of a mod menu type.
    fn result(&self, i: usize) -> bool {
        self.container.is_some() && self.results.contains(&i)
    }
    fn slot_max(&self, i: usize, stack: &ItemStack, rules: &dyn Rules) -> u32 {
        if self.worn(i).is_some_and(|k| k < 4) {
            return 1;
        }
        if self.container.is_none() && (5..=8).contains(&i) {
            return 1;
        }
        rules.max_stack(&stack.item).max(1)
    }
    /// The ranges quick move uses: (container start..end, main, hotbar).
    fn player_ranges(&self) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        match &self.container {
            Some(c) => (c.len()..c.len() + 27, c.len() + 27..c.len() + 36),
            None => (9..36, 36..45),
        }
    }
}

/// Inventory index of the n-th player slot of a container menu (27 main, then hotbar).
fn player_slot(n: usize) -> usize {
    if n < 27 { 9 + n } else { n - 27 }
}

/// Inventory.add-like merge of `stack` into slots `range` (moveItemStackTo).
fn move_to(
    view: &mut View,
    mut stack: ItemStack,
    range: std::ops::Range<usize>,
    reverse: bool,
    rules: &dyn Rules,
) -> Option<ItemStack> {
    let order: Vec<usize> = if reverse {
        range.rev().collect()
    } else {
        range.collect()
    };
    for &i in &order {
        if stack.count == 0 {
            break;
        }
        // A quick move tops up no result slot either, as a furnace's never
        // aims at its result (`AbstractFurnaceMenu.quickMoveStack`).
        if !view.enabled(i) || view.result(i) {
            continue;
        }
        if let Some(mut existing) = view.get(i)
            && existing.same_item_same_components(&stack)
        {
            let max = view.slot_max(i, &existing, rules);
            if existing.count < max {
                let moved = (max - existing.count).min(stack.count);
                existing.count += moved;
                stack.count -= moved;
                view.set(i, Some(existing));
            }
        }
    }
    for &i in &order {
        if stack.count == 0 {
            break;
        }
        if view.enabled(i) && view.get(i).is_none() && view.may_place(i, &stack, rules) {
            let moved = stack.count.min(view.slot_max(i, &stack, rules));
            view.set(
                i,
                Some(ItemStack {
                    count: moved,
                    ..stack.clone()
                }),
            );
            stack.count -= moved;
        }
    }
    (stack.count > 0).then_some(stack)
}

fn quick_move(view: &mut View, i: usize, rules: &dyn Rules) {
    let Some(stack) = view.get(i) else {
        return;
    };
    if !view.enabled(i) {
        return;
    }
    let (main, hotbar) = view.player_ranges();
    let rest = if let Some(c) = &view.container {
        let n = c.len();
        if i < n {
            // Container to inventory, filling from the hotbar's end.
            move_to(view, stack, n..n + 36, true, rules)
        } else {
            move_to(view, stack, 0..n, false, rules)
        }
    } else if (5..=8).contains(&i) || i == 45 || i < 5 {
        move_to(view, stack, 9..45, false, rules)
    } else {
        // Wear it, otherwise swap between the main inventory and the hotbar.
        let armor = rules
            .armor_index(&stack.item)
            .filter(|a| view.get(5 + a).is_none());
        if let Some(a) = armor {
            view.set(
                5 + a,
                Some(ItemStack {
                    count: 1,
                    ..stack.clone()
                }),
            );
            let mut rest = stack.clone();
            rest.count -= 1;
            (rest.count > 0).then_some(rest)
        } else if hotbar.contains(&i) {
            move_to(view, stack, main, false, rules)
        } else {
            move_to(view, stack, hotbar, false, rules)
        }
    };
    view.set(i, rest);
}

/// The click modes of ServerboundContainerClickPacket.
pub mod input {
    pub const PICKUP: i32 = 0;
    pub const QUICK_MOVE: i32 = 1;
    pub const SWAP: i32 = 2;
    pub const CLONE: i32 = 3;
    pub const THROW: i32 = 4;
    pub const QUICK_CRAFT: i32 = 5;
    pub const PICKUP_ALL: i32 = 6;
}

/// Apply one click; returns the stacks thrown out and the slots changed.
pub fn click(
    view: &mut View,
    drag: &mut Option<(u8, Vec<usize>)>,
    slot: i32,
    button: i32,
    mode: i32,
    rules: &dyn Rules,
) -> (Vec<ItemStack>, Vec<usize>) {
    let mut drops = Vec::new();
    let before: Vec<Option<ItemStack>> = (0..view.len()).map(|i| view.get(i)).collect();
    if mode == input::QUICK_CRAFT {
        quick_craft(view, drag, slot, button, rules);
    } else {
        *drag = None;
        match mode {
            input::PICKUP | input::QUICK_MOVE if slot == -999 => {
                if mode == input::PICKUP
                    && let Some(mut carried) = view.guest.carried.take()
                {
                    if button == 1 && carried.count > 1 {
                        let mut one = carried.clone();
                        one.count = 1;
                        carried.count -= 1;
                        view.guest.carried = Some(carried);
                        drops.push(one);
                    } else {
                        drops.push(carried);
                    }
                }
            }
            input::PICKUP => pickup(view, slot, button, rules),
            input::QUICK_MOVE => {
                if let Ok(i) = usize::try_from(slot)
                    && i < view.len()
                {
                    quick_move(view, i, rules);
                }
            }
            input::SWAP => swap(view, slot, button, rules),
            input::CLONE => {
                if view.guest.carried.is_none()
                    && view.guest.creative()
                    && let Ok(i) = usize::try_from(slot)
                    && let Some(stack) = view.get(i)
                {
                    let max = rules.max_stack(&stack.item).max(1);
                    view.guest.carried = Some(ItemStack {
                        count: max,
                        ..stack
                    });
                }
            }
            input::THROW => {
                if view.guest.carried.is_none()
                    && let Ok(i) = usize::try_from(slot)
                    && view.enabled(i)
                    && let Some(mut stack) = view.get(i)
                {
                    if button == 1 || stack.count <= 1 {
                        drops.push(stack);
                        view.set(i, None);
                    } else {
                        let mut one = stack.clone();
                        one.count = 1;
                        stack.count -= 1;
                        view.set(i, Some(stack));
                        drops.push(one);
                    }
                }
            }
            input::PICKUP_ALL => pickup_all(view, slot, rules),
            _ => {}
        }
    }
    let changed = (0..view.len())
        .filter(|&i| view.get(i) != before[i])
        .collect();
    (drops, changed)
}

fn pickup(view: &mut View, slot: i32, button: i32, rules: &dyn Rules) {
    let Ok(i) = usize::try_from(slot) else {
        return;
    };
    if i >= view.len() || !view.enabled(i) {
        return;
    }
    let right = button == 1;
    if bundle_click(view, i, right, rules) {
        return;
    }
    let (slot_item, carried) = (view.get(i), view.guest.carried.clone());
    match (slot_item, carried) {
        (None, None) => {}
        (None, Some(mut c)) => {
            if view.may_place(i, &c, rules) {
                let amount = if right { 1 } else { c.count }.min(view.slot_max(i, &c, rules));
                view.set(
                    i,
                    Some(ItemStack {
                        count: amount,
                        ..c.clone()
                    }),
                );
                c.count -= amount;
                view.guest.carried = (c.count > 0).then_some(c);
            }
        }
        (Some(mut s), None) => {
            let take = if right { s.count.div_ceil(2) } else { s.count };
            let taken = ItemStack {
                count: take,
                ..s.clone()
            };
            s.count -= take;
            view.set(i, (s.count > 0).then_some(s));
            view.guest.carried = Some(taken);
        }
        (Some(mut s), Some(mut c)) => {
            if !view.may_place(i, &c, rules) {
                // A slot that takes nothing gives onto the same carried item,
                // as far as the hand holds (`tryRemove`).
                if s.same_item_same_components(&c) {
                    let max = rules.max_stack(&c.item).max(1);
                    let moved = s.count.min(max.saturating_sub(c.count));
                    s.count -= moved;
                    c.count += moved;
                    view.set(i, (s.count > 0).then_some(s));
                    view.guest.carried = Some(c);
                }
                return;
            }
            if s.same_item_same_components(&c) {
                let max = view.slot_max(i, &s, rules);
                let moved = if right { 1 } else { c.count }.min(max.saturating_sub(s.count));
                s.count += moved;
                c.count -= moved;
                view.set(i, Some(s));
                view.guest.carried = (c.count > 0).then_some(c);
            } else if c.count <= view.slot_max(i, &c, rules) {
                view.set(i, Some(c));
                view.guest.carried = Some(s);
            }
        }
    }
}

/// `tryItemClickBehaviourOverride`: the carried stack's
/// `BundleItem.overrideStackedOnOther`, then the clicked one's
/// `overrideOtherStackedOnMe`. True when a bundle took the click.
fn bundle_click(view: &mut View, i: usize, right: bool, rules: &dyn Rules) -> bool {
    let items = rules.items();
    let other = view.get(i);
    if let Some(mut carried) = view.guest.carried.clone()
        && let Some(mut contents) = Contents::of(items, &carried)
        && other.is_some() != right
    {
        match other {
            // tryTransfer: as much as fits, through `safeTake`.
            Some(mut other) => {
                if can_be_in_bundle(&other) {
                    let amount = other.count.min(contents.room_for(items, &other));
                    if amount > 0 {
                        let mut taken = other.clone();
                        taken.count = amount;
                        other.count -= amount;
                        view.set(i, (other.count > 0).then_some(other));
                        contents.try_insert(items, &mut taken);
                    }
                }
            }
            // removeOne into the empty slot (`safeInsert`); the rest goes back.
            None => {
                if let Some(mut stack) = contents.remove_one(items) {
                    if view.may_place(i, &stack, rules) {
                        let amount = stack.count.min(view.slot_max(i, &stack, rules));
                        view.set(
                            i,
                            Some(ItemStack {
                                count: amount,
                                ..stack.clone()
                            }),
                        );
                        stack.count -= amount;
                    }
                    if stack.count > 0 {
                        contents.try_insert(items, &mut stack);
                    }
                }
            }
        }
        contents.store(&mut carried);
        view.guest.carried = Some(carried);
        return true;
    }
    let Some(mut bundle) = other else {
        return false;
    };
    let Some(mut contents) = Contents::of(items, &bundle) else {
        return false;
    };
    // Slot.allowModification: mayPickup and mayPlace of its item.
    let allowed = view.may_place(i, &bundle, rules);
    match (view.guest.carried.take(), right) {
        (Some(mut carried), false) => {
            if allowed {
                contents.try_insert(items, &mut carried);
            }
            view.guest.carried = (carried.count > 0).then_some(carried);
        }
        (None, true) => {
            if allowed {
                view.guest.carried = contents.remove_one(items);
            }
        }
        (carried, _) => {
            // toggleSelectedItem(-1), then the click goes on.
            view.guest.carried = carried;
            toggle_selected(&mut bundle, -1);
            view.set(i, Some(bundle));
            return false;
        }
    }
    contents.store(&mut bundle);
    view.set(i, Some(bundle));
    true
}

fn swap(view: &mut View, slot: i32, button: i32, rules: &dyn Rules) {
    let Ok(i) = usize::try_from(slot) else {
        return;
    };
    if i >= view.len() || !view.enabled(i) {
        return;
    }
    // Hotbar slot `button` (0..9) or the off hand (40). A chest has no
    // off-hand slot, so F swaps with `Guest.offhand` (Inventory slot 40).
    if button == 40 && view.container.is_some() {
        let (clicked, off) = (view.get(i), view.guest.offhand.clone());
        if off
            .as_ref()
            .is_some_and(|off| !view.may_place(i, off, rules))
        {
            return;
        }
        view.set(i, off);
        view.guest.offhand = clicked;
        return;
    }
    let other = match button {
        0..=8 => match &view.container {
            Some(c) => c.len() + 27 + button as usize,
            None => 36 + button as usize,
        },
        40 => 45,
        _ => return,
    };
    if other == i {
        return;
    }
    let (a, b) = (view.get(i), view.get(other));
    if a.as_ref().is_some_and(|a| !view.may_place(other, a, rules))
        || b.as_ref().is_some_and(|b| !view.may_place(i, b, rules))
    {
        return;
    }
    view.set(i, b);
    view.set(other, a);
}

fn pickup_all(view: &mut View, slot: i32, rules: &dyn Rules) {
    let _ = slot;
    let Some(mut carried) = view.guest.carried.clone() else {
        return;
    };
    let max = rules.max_stack(&carried.item).max(1);
    // Partial stacks first, then full ones.
    for full in [false, true] {
        for i in 0..view.len() {
            if carried.count >= max {
                break;
            }
            if !view.enabled(i) || (view.container.is_none() && i == 0) {
                continue;
            }
            let Some(mut s) = view.get(i) else {
                continue;
            };
            if !s.same_item_same_components(&carried) || (s.count == max) != full {
                continue;
            }
            let moved = (max - carried.count).min(s.count);
            carried.count += moved;
            s.count -= moved;
            view.set(i, (s.count > 0).then_some(s));
        }
    }
    view.guest.carried = Some(carried);
}

/// Dragging a stack over slots (QUICK_CRAFT): start, add slots, end.
fn quick_craft(
    view: &mut View,
    drag: &mut Option<(u8, Vec<usize>)>,
    slot: i32,
    button: i32,
    rules: &dyn Rules,
) {
    let (stage, kind) = (button & 3, ((button >> 2) & 3) as u8);
    match stage {
        0 => *drag = view.guest.carried.is_some().then_some((kind, Vec::new())),
        1 => {
            if let (Some((_, slots)), Ok(i)) = (drag.as_mut(), usize::try_from(slot))
                && i < view.len()
                && !slots.contains(&i)
                && let Some(c) = &view.guest.carried
                && view.enabled(i)
                && view.may_place(i, c, rules)
                && view.get(i).is_none_or(|s| s.same_item_same_components(c))
            {
                slots.push(i);
            }
        }
        _ => {
            let Some((kind, slots)) = drag.take() else {
                return;
            };
            let Some(mut carried) = view.guest.carried.clone() else {
                return;
            };
            if slots.is_empty() {
                return;
            }
            if slots.len() == 1 {
                // A single slot behaves like a plain click.
                let button = if kind == 1 { 1 } else { 0 };
                pickup(view, slots[0] as i32, button, rules);
                return;
            }
            let each = match kind {
                0 => carried.count / slots.len() as u32,
                1 => 1,
                _ => rules.max_stack(&carried.item).max(1),
            };
            for i in slots {
                if carried.count == 0 {
                    break;
                }
                let existing = view.get(i).map_or(0, |s| s.count);
                let max = view.slot_max(i, &carried, rules);
                let put = each.min(max.saturating_sub(existing)).min(carried.count);
                if put == 0 {
                    continue;
                }
                view.set(
                    i,
                    Some(ItemStack {
                        count: existing + put,
                        ..carried.clone()
                    }),
                );
                if kind != 2 || !view.guest.creative() {
                    carried.count -= put;
                }
            }
            view.guest.carried = (carried.count > 0).then_some(carried);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lan::{Joined, PlayerSender};
    use horizon_protocol::packets::configuration::ClientInformation;
    use horizon_world::loot::PackOverlay;
    use std::sync::{Arc, atomic::AtomicI32, mpsc};

    struct ItemRules(Items);
    impl Rules for ItemRules {
        fn items(&self) -> &Items {
            &self.0
        }
        fn max_stack(&self, item: &str) -> u32 {
            self.0.get(item).map_or(64, |i| i.max_stack)
        }
        fn armor_index(&self, _: &str) -> Option<usize> {
            None
        }
    }

    fn guest() -> Guest {
        let (tx, _rx) = mpsc::channel();
        Guest::new(
            Joined {
                conn: 2,
                uuid: 2,
                name: "Alex".into(),
                properties: Vec::new(),
                info: ClientInformation::new(8),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
            },
            2,
            [0.0, 64.0, 0.0],
            8,
        )
    }

    /// The prepared resources' items, or None (the test is skipped) without them.
    fn rules() -> Option<ItemRules> {
        let root = std::env::var_os("HORIZON_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            });
        if !root.join("items.json").exists() {
            eprintln!("skipping resource-backed menu check: {}", root.display());
            return None;
        }
        let registry = horizon_world::level::data::load_registry(&root).unwrap();
        let facts = crate::blocks::BlockFacts::load(&root, &registry).unwrap();
        Some(ItemRules(
            Items::load(&root, &registry, &facts, &PackOverlay::NONE).unwrap(),
        ))
    }

    fn contents(stack: &Option<ItemStack>) -> Vec<(String, u32)> {
        let inside = stack.as_ref().and_then(|s| s.data());
        inside.map_or(Vec::new(), |d| {
            d.bundle_contents
                .iter()
                .map(|s| (s.item.to_string(), s.count))
                .collect()
        })
    }

    /// A guest's chest clicks run `BundleItem`'s overrides: a carried
    /// bundle takes a slot's stack and drops its first one into an empty
    /// slot; a stack clicked onto a bundle goes in, as far as it fits.
    #[test]
    fn guest_chest_clicks_fill_and_empty_bundles() {
        let Some(rules) = rules() else {
            return;
        };
        let mut g = guest();
        let mut chest = vec![
            Some(ItemStack::new("minecraft:stone", 48)),
            None,
            Some(ItemStack::new("minecraft:bundle", 1)),
        ];
        let mut drag = None;
        let mut view = View {
            guest: &mut g,
            container: Some(&mut chest),
            shulker: false,
            hidden: &[],
            results: &[],
            equip: 0,
            accepts: &[],
        };
        view.guest.carried = Some(ItemStack::new("minecraft:bundle", 1));
        // Left click with the bundle: the stone goes in.
        click(&mut view, &mut drag, 0, 0, input::PICKUP, &rules);
        assert_eq!(view.get(0), None);
        assert_eq!(
            contents(&view.guest.carried),
            [("minecraft:stone".into(), 48)]
        );
        // Right click on an empty slot: the stone comes back out.
        click(&mut view, &mut drag, 1, 1, input::PICKUP, &rules);
        assert_eq!(view.get(1).map(|s| s.count), Some(48));
        assert_eq!(contents(&view.guest.carried), []);
        // Ender pearls (1/16 each) onto the bundle in the chest: 16 fit.
        view.guest.carried = Some(ItemStack::new("minecraft:ender_pearl", 16));
        click(&mut view, &mut drag, 2, 0, input::PICKUP, &rules);
        assert_eq!(view.guest.carried, None);
        let bundle = view.get(2);
        assert_eq!(contents(&bundle), [("minecraft:ender_pearl".into(), 16)]);
        // Full: more stone stays in hand.
        view.guest.carried = Some(ItemStack::new("minecraft:stone", 5));
        click(&mut view, &mut drag, 2, 0, input::PICKUP, &rules);
        assert_eq!(view.guest.carried.as_ref().map(|s| s.count), Some(5));
        // Right click on it with an empty hand takes the pearls out.
        view.guest.carried = None;
        click(&mut view, &mut drag, 2, 1, input::PICKUP, &rules);
        assert_eq!(
            view.guest.carried.as_ref().map(|s| (&*s.item, s.count)),
            Some(("minecraft:ender_pearl", 16))
        );
        assert_eq!(contents(&view.get(2)), []);
    }

    /// A mod container saved with more items than its menu has slots: the
    /// menu reads and writes only its count, the rest stay saved.
    #[test]
    fn mod_menu_shows_only_its_container_slots() {
        use crate::containers::{ModBlockEntity, ModContainer};
        let mut world = World::new(horizon_world::registry::BlockStateId(0), 0);
        let container = ModContainer {
            slots: 5,
            ..Default::default()
        };
        world
            .block_entities
            .set_mod_containers([(0, Arc::new(container))].into_iter().collect());
        let mut items = vec![None; 9];
        items[1] = Some(ItemStack::new("minecraft:stone", 3));
        items[7] = Some(ItemStack::new("minecraft:iron_ore", 2));
        let k = (0, 64, 0);
        world.block_entities.insert(
            k,
            BlockEntity::Mod(Box::new(ModBlockEntity {
                id: "gtmod:machine".into(),
                items,
                ..Default::default()
            })),
        );
        let mut g = guest();
        let slots = Slots::Block(k);
        let mut read = slots.read(&world, &g).unwrap();
        assert_eq!(read.len(), 5);
        assert_eq!(slots.get(&world, &g, 7), None);
        // A shift-click from the hotbar (menu slot 5 + 27) lands in the
        // container's first empty slot, not past its count.
        if let Some(rules) = rules() {
            g.inventory[0] = Some(ItemStack::new("minecraft:dirt", 1));
            let mut drag = None;
            let mut view = View {
                guest: &mut g,
                container: Some(&mut read),
                shulker: false,
                hidden: &[],
                results: &[],
                equip: 0,
            accepts: &[],
            };
            click(&mut view, &mut drag, 5 + 27, 0, input::QUICK_MOVE, &rules);
            assert_eq!(view.get(0).map(|s| s.item), Some("minecraft:dirt".into()));
        } else {
            read[0] = Some(ItemStack::new("minecraft:dirt", 1));
        }
        slots.write(&mut world, &mut g, read);
        let Some(BlockEntity::Mod(m)) = world.block_entities.get(&k) else {
            panic!("not a mod's");
        };
        assert_eq!(m.items.len(), 9);
        assert_eq!(
            m.items[0].as_ref().map(|s| &*s.item),
            Some("minecraft:dirt")
        );
        assert_eq!(m.items[1].as_ref().map(|s| s.count), Some(3));
        assert_eq!(m.items[7].as_ref().map(|s| s.count), Some(2));
    }

    /// A slot a mod's screen hides is not in the menu: a shift-click from the
    /// inventory skips it, and a shift-click on it moves nothing.
    #[test]
    fn a_hidden_slot_takes_no_quick_move() {
        let Some(rules) = rules() else {
            return;
        };
        let mut g = guest();
        let mut container = vec![None; 3];
        container[2] = Some(ItemStack::new("minecraft:stone", 1));
        g.inventory[0] = Some(ItemStack::new("minecraft:dirt", 1));
        let hidden = [0, 2];
        let mut drag = None;
        let mut view = View {
            guest: &mut g,
            container: Some(&mut container),
            shulker: false,
            hidden: &hidden,
            results: &[],
            equip: 0,
            accepts: &[],
        };
        // The hotbar's first slot (menu slot 3 + 27) lands in the one slot shown.
        click(&mut view, &mut drag, 3 + 27, 0, input::QUICK_MOVE, &rules);
        assert!(view.get(0).is_none());
        assert_eq!(view.get(1).map(|s| s.item), Some("minecraft:dirt".into()));
        // Nothing leaves a hidden slot either.
        click(&mut view, &mut drag, 2, 0, input::QUICK_MOVE, &rules);
        assert_eq!(view.get(2).map(|s| s.item), Some("minecraft:stone".into()));
    }

    /// A mod menu type's result slot takes nothing in, a click or a
    /// shift-click alike, even onto the same item, and gives what it holds
    /// onto the same carried item (`AbstractContainerMenu.doClick`).
    #[test]
    fn a_result_slot_is_take_only() {
        let Some(rules) = rules() else {
            return;
        };
        let mut g = guest();
        let mut container = vec![None, Some(ItemStack::new("minecraft:iron_ingot", 1))];
        g.inventory[0] = Some(ItemStack::new("minecraft:iron_ingot", 3));
        g.carried = Some(ItemStack::new("minecraft:iron_ingot", 2));
        let results = [1];
        let mut drag = None;
        let mut view = View {
            guest: &mut g,
            container: Some(&mut container),
            shulker: false,
            hidden: &[],
            results: &results,
            equip: 0,
            accepts: &[],
        };
        // The hotbar's first slot (menu slot 2 + 27) goes to slot 0, not onto the result.
        click(&mut view, &mut drag, 2 + 27, 0, input::QUICK_MOVE, &rules);
        assert_eq!(view.get(0).map(|s| s.count), Some(3));
        assert_eq!(view.get(1).map(|s| s.count), Some(1));
        // The carried ingots take the result's one instead of going in.
        click(&mut view, &mut drag, 1, 0, input::PICKUP, &rules);
        assert_eq!(view.get(1), None);
        assert_eq!(view.guest.carried.as_ref().map(|s| s.count), Some(3));
        // Nor does an empty result take any.
        click(&mut view, &mut drag, 1, 0, input::PICKUP, &rules);
        assert_eq!(view.get(1), None);
    }

    struct SwapRules;
    impl Rules for SwapRules {
        fn items(&self) -> &Items {
            unreachable!("swap does not read the item catalog");
        }
        fn max_stack(&self, _: &str) -> u32 {
            64
        }
        fn armor_index(&self, _: &str) -> Option<usize> {
            None
        }
    }

    /// A result slot takes nothing from a swap, F or a drag, but gives to a
    /// swap into an empty hand and to a double click's collect
    /// (`canTakeItemForPickAll` is true for it, as for a furnace's result).
    #[test]
    fn a_result_slot_takes_no_swap_or_drag() {
        let rules = SwapRules;
        let mut g = guest();
        let ingots = |n| Some(ItemStack::new("minecraft:iron_ingot", n));
        let mut container = vec![None, ingots(2), None];
        g.inventory[0] = Some(ItemStack::new("minecraft:stone", 1));
        g.offhand = Some(ItemStack::new("minecraft:stone", 1));
        let results = [1];
        let mut drag = None;
        let mut view = View {
            guest: &mut g,
            container: Some(&mut container),
            shulker: false,
            hidden: &[],
            results: &results,
            equip: 0,
            accepts: &[],
        };
        // A hotbar key and F over the result: the stone goes nowhere.
        click(&mut view, &mut drag, 1, 0, input::SWAP, &rules);
        click(&mut view, &mut drag, 1, 40, input::SWAP, &rules);
        assert_eq!(view.get(1), ingots(2));
        assert!(view.guest.offhand.is_some() && view.guest.inventory[0].is_some());
        // F with an empty off hand takes the ingots out.
        view.guest.offhand = None;
        click(&mut view, &mut drag, 1, 40, input::SWAP, &rules);
        assert_eq!((view.get(1), view.guest.offhand.clone()), (None, ingots(2)));
        // A drag over slots 0 to 2 passes the result by.
        view.guest.carried = Some(ItemStack::new("minecraft:dirt", 4));
        for (slot, button) in [(-999, 0), (0, 1), (1, 1), (2, 1), (-999, 2)] {
            click(
                &mut view,
                &mut drag,
                slot,
                button,
                input::QUICK_CRAFT,
                &rules,
            );
        }
        let count = |s: Option<ItemStack>| s.map(|s| s.count);
        assert_eq!(
            [view.get(0), view.get(1), view.get(2)].map(count),
            [Some(2), None, Some(2)]
        );
        // A double click collects from it.
        view.set(1, ingots(3));
        view.guest.carried = ingots(1);
        click(&mut view, &mut drag, 0, 0, input::PICKUP_ALL, &rules);
        assert_eq!((view.get(1), view.guest.carried.clone()), (None, ingots(4)));
    }

    /// F (SWAP button 40) over a chest or the inventory below it swaps with
    /// the off hand, even though the menu has no off-hand slot.
    #[test]
    fn guest_chest_f_swaps_with_the_off_hand() {
        let rules = SwapRules;
        let mut g = guest();
        g.offhand = Some(ItemStack::new("minecraft:shield", 1));
        let mut chest = vec![Some(ItemStack::new("minecraft:diamond", 1)), None];
        let mut drag = None;
        let mut view = View {
            guest: &mut g,
            container: Some(&mut chest),
            shulker: false,
            hidden: &[],
            results: &[],
            equip: 0,
            accepts: &[],
        };
        click(&mut view, &mut drag, 0, 40, input::SWAP, &rules);
        assert_eq!(
            view.get(0).as_ref().map(|s| &*s.item),
            Some("minecraft:shield")
        );
        assert_eq!(
            view.guest.offhand.as_ref().map(|s| &*s.item),
            Some("minecraft:diamond")
        );
        // Empty chest slot: the off hand goes in.
        click(&mut view, &mut drag, 1, 40, input::SWAP, &rules);
        assert_eq!(
            view.get(1).as_ref().map(|s| &*s.item),
            Some("minecraft:diamond")
        );
        assert!(view.guest.offhand.is_none());
        // F over the first hotbar slot (menu slot 2 + 27).
        view.guest.offhand = Some(ItemStack::new("minecraft:oak_boat", 1));
        view.guest.inventory[0] = Some(ItemStack::new("minecraft:apple", 1));
        click(&mut view, &mut drag, 2 + 27, 40, input::SWAP, &rules);
        assert_eq!(
            view.guest.inventory[0].as_ref().map(|s| &*s.item),
            Some("minecraft:oak_boat")
        );
        assert_eq!(
            view.guest.offhand.as_ref().map(|s| &*s.item),
            Some("minecraft:apple")
        );
    }

    #[test]
    fn guest_shulker_box_menu_refuses_shulker_boxes() {
        let Some(rules) = rules() else {
            return;
        };
        for shulker in [true, false] {
            let mut g = guest();
            let mut container = vec![None; 27];
            let mut drag = None;
            let mut view = View {
                guest: &mut g,
                container: Some(&mut container),
                shulker,
                hidden: &[],
                results: &[],
                equip: 0,
            accepts: &[],
            };
            // Carried onto an empty slot, shift-clicked from the hotbar
            // (menu slot 27 + 27) and swapped in with the number key 1.
            view.guest.carried = Some(ItemStack::new("minecraft:red_shulker_box", 1));
            click(&mut view, &mut drag, 0, 0, input::PICKUP, &rules);
            assert_eq!(view.get(0).is_none(), shulker);
            view.guest.carried = None;
            view.guest.inventory[0] = Some(ItemStack::new("minecraft:shulker_box", 1));
            click(&mut view, &mut drag, 54, 0, input::QUICK_MOVE, &rules);
            assert_eq!(view.guest.inventory[0].is_some(), shulker);
            view.guest.inventory[0] = Some(ItemStack::new("minecraft:blue_shulker_box", 1));
            click(&mut view, &mut drag, 5, 0, input::SWAP, &rules);
            assert_eq!(view.get(5).is_none(), shulker);
            // Anything else goes in.
            view.guest.carried = Some(ItemStack::new("minecraft:diamond", 1));
            click(&mut view, &mut drag, 9, 0, input::PICKUP, &rules);
            assert!(view.get(9).is_some());
        }
    }
}
