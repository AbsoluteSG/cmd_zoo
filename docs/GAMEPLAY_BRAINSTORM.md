# cmd_zoo — Gameplay Brainstorm

## What's Already There

The foundation is solid and further along than most prototypes:

- **Passive income loop** — animals produce coins/DNA accrued offline, redeemed on click
- **Breeding with genetics** — cross-species pairs roll weighted hybrid outcomes; rare hybrids grant DNA and codex entries
- **Two currencies** — coins (everyday) and DNA Helix (exotic/premium)
- **Food economy** — structures (Hay Bale → Aquaculture) produce food as a third resource, currently disconnected from animal-level needs
- **Walkable avatar** in an isometric zoo — WASD movement, collision, behavior chain
- **Timed systems** — habitat upgrades, breeding gestation, exotic shop windows (4h open / 45m closed)
- **Co-op networking** — host/visitor sessions, gift economy, Steam relay
- **Persistence** — 12 migration versions, robust offline accrual math

The food structures exist but their output goes nowhere interesting yet. The avatar can walk but has no stats. The breeding codex is started but never pays off. These are the natural seams to build from.

---

## Genre Blend Framing

| Genre | What it contributes | What the player feels |
|---|---|---|
| Zoo Tycoon | Income loops, habitats, visitors | Satisfaction of building and optimizing |
| Cult of the Lamb | Follower needs, rituals, dark doctrine | Moral tension; caregiving under pressure |
| Don't Starve | Keeper stats, seasons, night danger | Urgency; resource scarcity; survival dread |
| Oxygen Not Included | Resource pipelines, environment parameters, acolyte task priorities | Systemic satisfaction — fixing cascades before they propagate |
| Monster Hunter World | Monster ecology, field tracking, material crafting loops | Ownership and expertise — you *know* your animals |
| League of Legends | Kit/role identity, species synergies, map objectives, power spikes | Strategic composition — your zoo has a playstyle |

The blend to aim for: **a zoo that feels alive and slightly threatening**. Your animals aren't just revenue units — they have morale, and so do you. At night the zoo changes. Seasons stress your plans. Visitors become devotees. Hybrids unlock secrets.

---

## Core Design Principle

> Every existing system should feed at least two loops.
> Food feeds animals AND the keeper.
> DNA funds exotic animals AND dark rituals.
> Hybrid discoveries earn codex entries AND unlock doctrine pages.
> Materials from researched animals craft ritual components AND keeper equipment.

---

## Idea Clusters

### 1. Day / Night Cycle — the fundamental new axis

The isometric world already has a day-like feel. Split 24 real-minutes into a full in-game day:

- **Day phase** (~16 min): Zoo is open. Visitors generate income. Normal breeding, shopping, collecting.
- **Dusk** (~2 min): Visitors leave. An optional nightly task queue appears (rituals, feedings, inspections).
- **Night** (~6 min): Zoo is closed. Animals have needs. Strange events can fire. The keeper walks the grounds alone.

Night is where Don't Starve bleeds in — it should feel slightly uncomfortable. Torch/lantern structures push back the darkness. Unlit areas lower morale for whatever animals are housed there. The keeper's sanity drains slowly at night if idle.

### 2. Keeper Stats — Sanity & Hunger

The avatar is already a physical presence in the world but has no stakes attached to it. Give the keeper two persistent meters:

**Hunger** — drains ~1 unit/minute (real time). Food structures already produce food; redirect some to be keeper-consumable. Different foods restore different amounts and add minor effects (berries = small sanity bump, feed mill output = bulk hunger, aquaculture fish = sanity restore + small DNA bonus). If hunger hits zero: movement slows, income collection range shrinks, eventually sanity starts draining faster.

**Sanity** — drains from: night darkness, low-morale animals nearby, financial losses, certain crisis events. Restores from: collecting from content animals, performing rituals, eating certain foods, building decorations. Low sanity cascades: visual distortions (Don't Starve style), prices appear wrong (you see 150 coins but it costs 200), morale is harder to read. Sanity zero = a "breakdown event" — not permadeath, but a hard consequence (see §8).

This makes food structures relevant and creates active keeper-management pressure alongside the passive animal economy.

### 3. Animal Morale — the hidden second economy

Right now animals are pure producers. Give each animal a **Morale** meter (0–100):

- Morale decays if: habitat is overcrowded, food is not available, animal is in the dark at night, habitat hasn't been upgraded recently, keeper hasn't visited in N days
- Morale grows from: keeper walking near the habitat, food being available, habitat upgraded recently, ritual effects, personality traits
- **Effect on income**: morale directly scales income. 100 morale = full rate. 50 morale = 75% rate. 0 morale = 25% rate and breeding disabled.
- **Morale crisis**: if an animal hits 0 morale, it enters an "Agitated" state — visible visual change, and it may attempt an escape event.

This makes the food economy meaningful (structures must produce enough to feed animals, not just sit idle), connects keeper movement to real game value (walking near habitats is now purposeful), and creates a tension between "big expensive zoo" and "zoo you can actually maintain."

### 4. Visitor Wonder — converting tourists into devotees

Visitors right now are co-op constructs (remote players). Extend to NPC visitors:

- Each day, N NPC visitors enter (scales with zoo reputation)
- Each visitor has a **Wonder** meter that fills as they walk past high-morale, well-kept habitats, rare species, and decorations
- Visitor exits with:
  - Low Wonder → standard ticket income
  - Medium Wonder → leaves a "Rave Review" (next-day visitor count +10%)
  - High Wonder → becomes a **Patron**: pays a monthly subscription, occasionally sends gifts
  - Max Wonder → can be recruited as a **Keeper Acolyte** (see §5)

Negatives: low-morale animals visible to visitors drain their Wonder. Escape events nuke every current visitor's Wonder to zero and may trigger an activist event.

### 5. Keeper Acolytes — follower management

When a visitor reaches max Wonder and you recruit them, they become an Acolyte. This is the Cult of the Lamb loop:

- Acolytes need: wages (coins/day), food, and purpose (an assigned task)
- **Tasks they can fill**: auto-collect from one habitat, maintain one structure, guard a gate (reduces escape probability), patrol the night paths (pushes back darkness, restores some keeper sanity)
- Acolytes have morale too — neglected acolytes lose faith, reduce their task efficiency, and eventually defect (takes their assigned habitat's animals on the way out if morale hits zero, for a dark twist)
- Max ~4–6 acolytes; this mirrors the existing nest-slot system

This creates a second caregiving layer on top of animals — you're now managing sentient followers, not just livestock. The moral texture of Cult of the Lamb comes from the fact that followers are both assets and beings with needs.

### 6. Ritual System — spending DNA for permanent power

DNA Helix currently buys exotic animals and skips the shop cooldown. Expand it:

**The Altar** — a new structure you build once (costs coins). At the altar you perform nightly rituals:

| Ritual | Cost | Effect |
|---|---|---|
| Feeding Rite | Food × 20 | All animals +15 morale for 24h |
| Breeding Moon | DNA × 10 | Hybrid chance doubled for this night's gestation |
| Dark Harvest | Sacrifice 1 animal | Owner habitat gains permanent +5% income rate |
| Visitor Blessing | DNA × 5 | Next day's visitors all start at 50 Wonder |
| Keeper's Feast | Food × 30 | Full hunger + sanity restore for the keeper |
| Extinction Rite | Sacrifice a hybrid | Unlock a new doctrine page in the Grimoire |

The Dark Harvest and Extinction Rite are the moral pressure valves. The game never forces you to use them; they're always the most efficient option for their outcome. Do you sacrifice the rare tortoise to permanently boost the habitat? That's a meaningful choice when the tortoise has a personality and a morale history.

### 7. The Grimoire / Doctrine System

The existing breeding codex is half of this. Make it a full tome:

- **Codex section**: each hybrid discovered fills a page (already exists in spirit)
- **Ritual section**: each ritual performed fills a page
- **Doctrine pages**: filling complete chapters of either section unlocks a permanent **Doctrine** choice

Doctrines are permanent passive upgrades with character. Two choices offered per unlock, pick one:

- "Carnivore's Tithe" vs "Herbivore's Blessing" — boosts one food type's effect on animal morale
- "Open Gates" vs "Shrine Zoo" — more NPC visitors per day vs higher Patron conversion rate
- "The Long Night" vs "Eternal Noon" — night phase doubles in length (more ritual time, more dread) vs night phase halves (safer, less opportunity)

These should feel like building a character. By mid-game your zoo has an identity shaped by your doctrine choices.

### 8. Crisis Events — active disruption

Currently nothing breaks the passive income model. Inject randomized crises on a weighted schedule:

**Low severity (frequent):**
- Animal Won't Eat — a single animal drops to 0 food satisfaction; you have 30 min (real time) to resolve or it hits Agitated
- Habitat Wear — an old habitat needs a maintenance click or slowly loses 1 capacity slot
- Acolyte Grievance — an acolyte needs attention (talk, give a gift item, pay a bonus)

**Medium severity (occasional):**
- Escape Attempt — an Agitated animal breaks containment. Chase event in the isometric world; recapture within 5 in-game minutes or the animal is gone
- Journalist Visit — surprise inspection; zoo's visible morale average is scored. Result = multi-day income modifier
- Rival Zoo Poaching — a competing zoo tries to recruit one of your Acolytes with a better offer; respond with a counter-offer of coins/DNA or lose them

**High severity (rare):**
- Plague — spreads between habitats sharing a food source; quarantine mechanic (wall off with a temporary barrier, treat with medicine crafted from food + DNA), 24h containment window
- Eclipse Event — rare celestial event (seeded by calendar date, so all players share it). Night extends, hybrid chance triples, keeper sanity drains 3× faster. High-risk/reward window.
- The Investigator — arrives if you've used too many Dark Rituals. Inspects everything. Must have metrics above thresholds or lose a habitat.

### 9. Seasons — long-term pressure cycles

A season lasts N real days (configurable; maybe 7). Four seasons, each stressing different systems:

**Spring** — baseline. Breeding rates normal. Visitor counts medium.

**Summer** — visitor counts peak (+40%). Tropical animals thrive. Temperate and arctic animals lose morale 2× faster without cooling structures. Food spoils faster (capped storage on structures decreases).

**Autumn** — hybrid breeding windows are longer (animals more active). Exotic shop rotates with seasonal-exclusive species only available this season. Acolyte morale is high; they're content.

**Winter** — visitor counts drop (–50%). Tropical animals in unheated habitats enter hibernation (zero income). The night phase is longer and darker. Food structures produce 30% less. BUT: arctic species produce 50% more income and arctic hybrids are more common. The keeper's hunger drains faster.

Seasons create a planning problem: do you build a general zoo that survives all seasons, or specialize? That's a tycoon decision.

### 10. Personality Traits — individuality layer

Each animal gains 1–2 traits at birth (from breeding) or acquisition. Traits are stored in the save file:

| Trait | Effect |
|---|---|
| **Shy** | –20% income but never triggers escape events |
| **Gregarious** | +10% income per adjacent same-species animal |
| **Blessed** | +5% morale from all ritual effects |
| **Sprinter** | High income, low cap — surfaces the existing archetype as a visible label |
| **Corrupted** | Double income, but morale decays 2× faster and infects neighbors (–5 morale/hr to adjacent habitats) |
| **Curious** | 5% chance each collection to bring a random item (food, DNA, or a relic fragment) |
| **Ancient** | Found only on max-level animals — immune to morale decay, bonus sanity to keeper when nearby |

Traits make individual animals feel like characters rather than identical income units. A Corrupted animal is a moral choice: massive income but active harm to neighbors. An Ancient feels earned.

### 11. Exploration / Expeditions

The avatar can walk the zoo, but the map edge is a wall. Open it:

- Unlock an **Expedition Gate** at one map edge (costs coins + a completed Grimoire chapter)
- Send your avatar or an Acolyte on an expedition. Expedition has a real-time duration (30 min / 2h / 8h) and a risk level
- Returns with: rare animal egg, relic fragment, exotic food type, or a species not found anywhere in the shop
- If an Acolyte goes: they can fail (low-morale Acolyte) — returns empty or doesn't return at all (the Don't Starve "Wilson disappears into the dark" feeling)
- If the keeper goes: zoo is on autopilot; Acolytes manage it. Sanity drains slowly while away. Keeper returns with the spoils but to potentially worse zoo state.

This creates an absence mechanic — the zoo doesn't perfectly self-manage while you're exploring.

### 12. Relics — exhibit objects with ambient effects

Expedition returns or rare visitor gifts might include Relics: unique objects placed on the isometric grid as exhibit tiles (not habitats, not structures):

- Relics have no income but radiate a zone effect to nearby habitats (+morale, +Wonder to passing visitors, +breeding speed)
- Examples: Ancient Skull (+10 visitor Wonder in radius), Crystal Egg (nearby breeding finishes 20% faster), Shepherd's Crook (nearby acolytes have +10 morale)
- Relics are not craftable, not purchasable — expedition-only or rare event drops
- Place limit: 6 relics (prevents stacking)

This gives expeditions a tangible spatial payoff and rewards players who explore the map physically.

### 13. The Night Market — expanding the exotic shop

The existing Exotic Shop already has a clever timed window mechanic. Push it further:

- Rename and reskin as the **Night Market** — only open during the night phase
- Day-based Exotic Shop stays for normal species; Night Market sells ritual components, relic fragments, forbidden hybrids, and seasonal-exclusive species
- The Night Market vendor is a recurring character (the Wandering Merchant) — different personality every appearance, sells one unique "weird" item per night in addition to the rotating catalog
- Mood system: buy consistently and the vendor offers better items; skip multiple nights and they show up with lower-tier goods

---

## Inspiration Layer 2 — ONI, Monster Hunter World & League of Legends

The original genre blend (Zoo Tycoon + Cult of the Lamb + Don't Starve) establishes the tension and caregiving loops. These three new references add depth at different layers: ONI contributes systemic infrastructure simulation, MHW contributes ecological identity and research-driven acquisition, and LoL contributes strategic composition and spatial objective thinking.

Each idea below notes which existing systems it connects to.

---

### From Oxygen Not Included — Systems Depth & Resource Pipelines

#### 14. Habitat Environment Parameters — Temperature & Humidity

Each habitat has two hidden environmental stats: **Temperature** and **Humidity**. These are shaped by the habitat's theme (Arctic = cold/dry, Ocean = cool/humid, Jungle = hot/humid), the current season, nearby structures (heaters, cooling fans, misters), and whether it's day or night.

Each species has a preferred temperature/humidity band. Animals outside their band suffer accelerated morale decay — proportional to how far outside the band they are, not binary. You can survive a slightly suboptimal habitat, but you'll be constantly fighting morale instead of coasting.

Environmental management structures (Heater, Cooling Fan, Misting System) would be the new counterpart to the existing food structures — they consume food/coins to run and must be placed near the habitats they serve.

*Connects to:* Animal Morale (§3), Habitat themes (existing — Arctic/Jungle/Ocean themes now have mechanical weight beyond cosmetics), Seasons (§9 — winter dramatically widens the cold/warm gap), Crisis Events (§8 — heatwave or cold snap events push parameters to extremes).

---

#### 15. Food Routing — Delivery Pipelines

Food doesn't auto-teleport to animals. Each food structure produces into a local inventory. Habitats are "fed" only if a delivery path exists: either the keeper manually runs food, an Acolyte has a delivery route, or the habitat is within the passive delivery radius of a structure.

Each food structure has a **delivery radius** (the tile count it can reach without an Acolyte). Build your zoo so food structures are close to the habitats they serve, or assign an Acolyte to bridge the gap. An Acolyte on food duty covers a wider radius but can only serve one structure's output.

The spatial implication: zoo layout now has a logistics layer. A beautiful symmetrical zoo with food production centralized may have habitats starving at the edges. A messier but efficient layout with food structures scattered close to habitat clusters works better.

*Connects to:* Food structures (existing — Hay Bale, Insectary, Feed Mill, Aquaculture now have spatial range), Animal Morale (§3 — hunger is what drives morale decay), Acolytes (§5 — delivery duty is now a real, spatial assignment), isometric grid (existing).

---

#### 16. Animal Behavioral Schedules

Each species has a daily schedule with three windows cycling through the in-game day:

- **Feeding window** (~4 in-game hours): high food consumption; morale rises if food is available, falls if not. Best time to trigger a Feeding Rite ritual for double effect.
- **Active window** (~8 in-game hours): peak income rate. Visitors observing the habitat during this window gain Wonder faster.
- **Rest window** (~12 in-game hours): income rate halved, but morale slowly recovers even without food. Visitors near a resting habitat gain Wonder more slowly.

Schedules are fixed per species but visible in the species dossier once researched. Some species are **Nocturnal** — their active window falls in the night phase. Managing nocturnal species means your night work has a payoff: they produce while diurnal species sleep.

*Connects to:* Day/Night cycle (§1 — nocturnal species reward night play), Animal Morale (§3), Ritual System (§6 — rituals timed to the feeding window are more efficient), Visitor Wonder (§4), Species Research Dossiers (§22 — schedules are a dossier milestone unlock).

---

#### 17. Environmental Cascade Failures

Small problems that compound if ignored — the signature ONI experience. Examples:

- **Flooding**: a water habitat left at max capacity during a rain event overflows to adjacent tiles. Adjacent habitats receive a Humidity spike; cold-sensitive species in those habitats lose morale rapidly.
- **Heatwave cascade**: in summer, if a tropical habitat has no cooling structure, its temperature climbs. Once it crosses a threshold, it radiates heat to neighbors. If a neighbor also has no cooling, both crash simultaneously.
- **Delivery collapse**: if the Acolyte on food delivery defects (low morale) and no replacement is assigned, all habitats on their route start starving simultaneously — a cascade of morale drops across the zoo.

The antidote is redundancy (multiple delivery routes, cooling structures on both sides of a heat source) and active monitoring. Fog of War (§31) makes these cascades extra dangerous at night when you can't see them developing.

*Connects to:* Crisis Events (§8), Habitat Environment Parameters (§14), Acolytes (§5 — single points of failure are now consequential), Day/Night cycle (§1 — most cascades start at night and are discovered at dawn), Fog of War (§31).

---

#### 18. Acolyte Skill Specialization

Acolytes build skills through repetition, ONI Duplicant-style. Each Acolyte has four skill tracks:

- **Animal Care**: improves from habitat collection and morale management. Higher skill = larger delivery radius, better morale-restoration aura when nearby.
- **Engineering**: improves from maintaining structures. Higher skill = structures produce more, environmental controls are more effective.
- **Research**: improves from working at the Grimoire/Altar. Higher skill = rituals are cheaper, dossier milestones unlock faster.
- **Patrol**: improves from night patrol duty. Higher skill = larger visibility radius, faster sanity restoration granted to the keeper.

Skills make Acolytes asymmetric over time. Your first Acolyte will naturally specialize in whatever you assign them first. The Rival Zoo Poaching event (§8) is now catastrophic if it targets your high-Animal-Care Acolyte — not just because you lose a body, but because you lose accumulated expertise.

*Connects to:* Acolytes (§5), Crisis Events (§8 — Rival Poaching now has real stakes), Ritual System (§6), Research Board (§19).

---

#### 19. Research Board — Infrastructure Tech Tree

Separate from the Grimoire (which tracks lore and doctrine), the Research Board is a pure infrastructure tech tree funded with food and DNA. Research projects include:

- New habitat capacity tiers and environment control slots
- Food processing recipes (combine two food types into a higher-yield compound feed)
- Keeper equipment (collection bag upgrade, a monocle tool that shows animal morale from a distance without walking to the habitat)
- Structure efficiency improvements (Aquaculture production +20%, Heater power draw –30%)
- Environmental control structures (Misting System, Radiant Heater — prerequisites for managing habitat parameters at scale)

Each project takes real time to complete. An Acolyte with high Research skill speeds this up. The Research Board connects long-term progression to active resource investment rather than just passive income scaling.

*Connects to:* Food structures (existing), DNA currency (existing), Grimoire/Doctrine (§7 — separate concerns: Grimoire = lore and moral choices, Research Board = pure capability), Acolyte Skill Specialization (§18 — Research skill reduces project times).

---

### From Monster Hunter World — Ecology, Tracking & Materials

#### 20. Behavioral Ecology Tags

Every species in the catalog is tagged with an ecological role: **Apex**, **Herbivore**, **Omnivore**, **Symbiont**, **Nocturnal**, **Scavenger**, or **Piscivore**. Species can carry multiple tags.

Adjacent habitats with compatible ecology tags grant passive bonuses:

- **Apex + Herbivore**: dramatic visitor Wonder boost (natural drama is captivating) but the Herbivore suffers a mild stress penalty (–10 morale/day) unless a Pacification Relic is placed between them
- **Two Symbionts adjacent**: both gain +10% income from mutualism
- **Scavenger near any Apex**: Scavenger income +15% (feeding off scraps from the dominant species)
- **Nocturnal + Diurnal**: no conflict, no synergy — they operate in different worlds and largely ignore each other

Incompatible pairings aren't necessarily bad choices. Apex + Herbivore is a calculated risk: maximum Wonder at the cost of ongoing morale management. This rewards ecological knowledge and makes habitat placement feel meaningful.

*Connects to:* Animal Morale (§3), Visitor Wonder (§4), isometric grid (existing — placement now has ecological weight), Species Synergy Combos (§27), Personality Traits (§10 — a "Shy" Herbivore adjacent to an Apex takes extra stress).

---

#### 21. Field Tracking — Pre-Acquisition Research

Some species — particularly exotics and rare hybrids — cannot be purchased outright. Before they appear in the shop, you must track them.

Expeditions (§11) occasionally return with **Field Signs**: footprints, shed materials, or ecological markings associated with an unknown species. Collect enough field signs of a specific type and the species "unlocks" — it begins appearing in the Exotic Shop or Night Market.

Field signs accumulate across multiple expeditions. Short expeditions return common signs; long expeditions return rare ones. Some species require signs from specific biome contexts (night expeditions return different sign types than day ones).

This delays exotic acquisition behind active play rather than just money. You can't rush it with DNA — you have to go out and look.

*Connects to:* Expeditions (§11), Species Research Dossiers (§22 — field signs pre-fill the first dossier page), Exotic Shop (existing — field sign gating controls catalog unlock order), Night Market (§13 — night expeditions unlock night market species).

---

#### 22. Species Research Dossiers

Each species you own accumulates a **Research Dossier** that fills in over time. Milestone unlocks at 30, 60, and 90 in-game days of ownership:

- **30 days**: preferred food type revealed (feeding this food boosts morale +10% beyond standard); optimal temperature/humidity band revealed
- **60 days**: best breeding partner revealed (the cross with highest hybrid chance); behavioral schedule revealed (§16); rare event hint ("this species occasionally exhibits bioluminescence at night")
- **90 days**: fully researched — species now produces a secondary material (§23), gains a +5% permanent income multiplier, and a rare behavioral event can trigger (a one-time spectacular visitor Wonder spike)

Dossiers make long-term animal ownership feel like genuine expertise. A fully researched Giant Tortoise you've had for months is worth more than a freshly purchased one — because you understand it.

*Connects to:* Species catalog (existing), Animal Morale (§3 — knowing preferred food reduces morale management effort), Breeding system (existing — dossier reveals optimal partner), Animal Behavioral Schedules (§16), Animal Material Economy (§23).

---

#### 23. Animal Material Economy

Fully researched animals (90-day dossier milestone) produce a secondary resource alongside coins — a **material** specific to their ecological type:

| Ecology type | Material | Example |
|---|---|---|
| Herbivore | Fiber (wool, fur, hide) | Field Mouse → Soft Pelt |
| Apex | Trophy components | Snow Lion → Mane Shard |
| Piscivore | Aquatic components | Aquaculture species → Iridescent Scale |
| Symbiont | Essences | Blue Frog → Toxin Gland |
| Scavenger | Mixed scraps | — |

Materials accumulate in a habitat-level inventory. Uses:
- **Craft ritual components** — spend materials instead of DNA for certain rituals (alternative resource path)
- **Craft habitat decorations** — minor morale boost, visitor Wonder bonus
- **Sell at Night Market** — the Wandering Merchant buys materials for DNA
- **Craft keeper equipment** via the Research Board (§19)

The material layer is a mid-game resource bridge: it gives an alternative path to DNA accumulation and creates a reason to keep researching even "ordinary" species.

*Connects to:* Species Research Dossiers (§22 — materials unlock at 90 days), Ritual System (§6 — materials as ritual cost alternative), Night Market (§13 — vendor buys materials), Research Board (§19 — materials craft keeper tools), DNA currency (existing).

---

#### 24. Ecological Turf Wars

When two Apex-tagged species are housed in adjacent habitats, periodic **Turf Conflict** events fire — a visible notification showing the two animals are agitated at each other across the fence. Both habitats take a short morale hit (–20 for 2 in-game hours).

Responses available:
- **Build a neutral barrier tile** between them: blocks the territorial line-of-sight, eliminates turf events, but reduces visitor Wonder slightly — visitors can no longer see both at once
- **Pacification Ritual** at the Altar: costs food × 10, suppresses turf events for one season
- **Relocate one habitat** on the grid to break adjacency
- **Accept the cost**: turf events briefly spike visitor Wonder dramatically (visitors love the drama) before the morale drop resolves — high-risk, high-reward for a Wonder-maximizing zoo

*Connects to:* Behavioral Ecology Tags (§20), Crisis Events (§8 — turf war is a medium-severity event), Ritual System (§6), Visitor Wonder (§4), Animal Morale (§3), isometric grid (existing — adjacency has consequences now).

---

#### 25. Seasonal Megafauna Encounters

Once per season, a rare **Wandering Animal** appears at the expedition gate — visible at the map edge but not inside the zoo. It's a creature tied to the current season's ecology.

You have one in-game day to act:
- **Observe**: passively watch it, earning 2–3 field sign pages for its dossier (free research progress)
- **Lure**: drop a specific food type it favors (revealed in its partial dossier if you've tracked it before) — increases capture chance
- **Capture**: attempt to bring it in; requires its dossier to be at least 30% complete; failure means it flees and won't return this season
- **Do nothing**: it leaves at day's end; if it's the last season it could spawn, the species may retire from encounters for a long time

Miss the encounter three seasons in a row and a "Last Sighting" event fires — a final opportunity at increased capture difficulty.

*Connects to:* Seasons (§9), Expeditions (§11), Field Tracking (§21), Species Research Dossiers (§22), Exotic Shop (existing — captured megafauna are one-of-a-kind, not re-purchasable), Crisis Events (§8 — "Last Sighting" is a time-pressure event).

---

### From League of Legends — Kit Identity, Synergies & Objectives

#### 26. Species Role Identity

Formalize the existing income archetypes (Sprinter, Tank, Balanced) into named **Roles** visible in the UI — the zoo equivalent of champion roles:

| Role | Income profile | Needs | Strength |
|---|---|---|---|
| **Anchor** | Low rate, enormous cap | Low food, low keeper attention | Survives crises; income keeps flowing when you're overwhelmed |
| **Showpiece** | High rate, low cap | High food, regular keeper visits | Maximum income when maintained; collapses when neglected |
| **Catalyst** | Low self-income | Minimal | Boosts adjacent species' income/morale by 15–25% |
| **Scout** | Moderate, volatile | Expedition-only acquisition | Carries expedition bonuses back to the zoo as passive effects |
| **Lure** | Low-to-moderate | Moderate | Pulls visitor pathfinding toward their zone; nearby habitats gain +Wonder |

A zoo built around Showpieces with one Catalyst adjacent to each and an Anchor in the corner is a **composition** — the zoo equivalent of a team draft. Pure Showpiece spam collapses in crises. Pure Anchors are safe and profitable but boring. The interesting zoos are compositions.

*Connects to:* Existing Sprinter/Tank/Balanced archetypes (now surfaced as visible roles rather than hidden income scaling math), Animal Morale (§3 — Showpieces punish neglect hardest), Visitor Wonder (§4 — Lures have spatial effect on visitor pathing), Behavioral Ecology Tags (§20).

---

#### 27. Species Synergy Combos

Discoverable pairings that grant zoo-wide or adjacency bonuses when certain species co-exist. These are logged in the Grimoire as unlockable entries — not announced in the shop:

| Synergy | Requirement | Effect |
|---|---|---|
| Wetland Harmony | Field Mouse + Blue Frog in adjacent habitats | +15% income for both |
| Apex Dominion | Any two Apex species in the same zoo | +20% morale for all arctic/apex habitats |
| Support Network | Any three Catalyst species in the zoo | Acolytes gain +10 passive morale/day |
| Night Court | Three or more Nocturnal species in the zoo | +25% income during the night phase |
| Predator's Halo | Apex adjacent to any Herbivore | Herbivore income +30% (adrenaline response) — stress penalty still applies |
| Ancient Lineage | Any fully-researched species + an Ancient-trait animal in the zoo | Dossier research speed +20% for all other species |
| Expedition Bond | A Scout species + its expedition biome origin represented in a nearby Relic | Scout income +40% |

Synergies are discovered when the combination is achieved in your zoo, or hinted at through dossier research milestones. Finding a non-obvious synergy feels like discovering the meta.

*Connects to:* Grimoire/Doctrine (§7 — synergies are Grimoire entries), Behavioral Ecology Tags (§20), Species Role Identity (§26), Personality Traits (§10 — Ancient trait feeds "Ancient Lineage"), Relics (§12).

---

#### 28. Map Objectives — Landmark Structures

High-cost buildable structures that require good grid positioning and grant zoo-wide effects. Only a few can fit on a 16×16 map, so each one is a meaningful trade-off against habitat space. Unlike normal structures, Landmarks cannot be upgraded or sold once placed:

| Landmark | Placement requirement | Cost | Effect |
|---|---|---|---|
| **Waterfall Feature** | Adjacent to any aquatic habitat | 800 coins + 5 Aquatic materials | Zoo-wide +10 visitor Wonder; passive sanity regen for keeper nearby |
| **Ancient Grove** | Center 4×4 of the map | 1,200 coins + rare Wood material | All animals' morale decay rate halved during Spring/Autumn |
| **Moon Pool** | Open tile, built at night | 300 coins + 30 DNA | Enables moon-phase rituals; +1 passive DNA/hour at night |
| **Observation Tower** | Map edge tile | 600 coins | Reveals all Wild Camp locations (§32); reduces expedition duration by 20% |
| **Bone Archway** | Entrance tile | 400 coins + 2 Trophy materials | All entering visitors start at 40 Wonder (vs. default 0) |

Landmark placement is permanent and competes for the same grid as habitats. If you place the Ancient Grove off-center to make room for a habitat, you lose some of its radius effect. These decisions shape the zoo's physical identity.

*Connects to:* Isometric grid (existing — 2×2 footprint, same as habitats), Ritual System (§6 — Moon Pool enables a new ritual tier), Expeditions (§11 — Observation Tower synergy), Visitor Wonder (§4), Animal Morale (§3 — Ancient Grove reduces pressure), DNA currency (existing).

---

#### 29. Power Spike Profiles

Formalize when each species is at its best — visible in the species dossier as a readable Power Curve:

- **Early-spike**: strong income at L1–4, plateaus hard at L5+. Best for early-game funding; replace or deprioritize once you can.
- **Mid-scaler**: average throughout, peaks at L6–8. The workhorses of a stable zoo.
- **Late-carry (Hypercarry)**: terrible income at L1–6, becomes the best income source in the game at L9–10. Requires massive sustained investment — coins, food, and a dedicated Acolyte. The reward for patience and planning.
- **Flat-carry (Sustained)**: consistent across all levels; never exceptional, never falls off. The Anchor archetype's income version — reliable crisis-proofing.

Players who know the curves can sequence upgrades intentionally: rush an Early-spike to fund the zoo, then invest into a Hypercarry for late-game scaling, with a Flat-carry as a foundation throughout. The curves are read from the species dossier, so researching an animal also teaches you how to maximize it.

*Connects to:* Existing Sprinter/Tank/Balanced archetypes (now visible instead of implicit), Habitat upgrade system (existing), Species Role Identity (§26 — Hypercarry is the Showpiece taken to an extreme), Research Board (§19 — a new capacity tier may be required before a Hypercarry's L10 is achievable).

---

#### 30. Zoo Expo — Curation & Draft Events

Every two seasons, a **Zoo Expo** event is announced with a 2-day lead time. The prompt varies:

- "Best Nocturnal Collection" — score highest with multiple nocturnal species, high morale, and the Night Court synergy discovered
- "Rarest Specimen Showcase" — score on hybrid rarity and expedition-only species
- "Community Harmony" — score on average morale across all habitats, rewarding breadth over any single spectacular animal

You select a **lineup of 5 species** to feature. The selection process is the draft: your rarest hybrid earns the highest individual score, but if an escape event fires during the Expo day, the featured animal is at risk. A safer lineup of well-managed Catalysts and Anchors is lower-ceiling but insulated from crisis.

Scoring rewards: a wave of high-Wonder visitors the following week, an exclusive doctrine option, seasonal animal unlocks not available any other way, and the Wandering Merchant brings a premium Night Market inventory that night.

*Connects to:* Seasons (§9), Visitor Wonder (§4), Grimoire/Doctrine (§7 — exclusive doctrine option), Crisis Events (§8 — escape during Expo is catastrophic), Species Synergy Combos (§27 — matching the Expo prompt's synergy gives a score bonus), Night Market (§13).

---

#### 31. Fog of War — Keeper Presence as Visibility

Areas of the isometric grid that the keeper hasn't visited recently (or that no Acolyte patrols) gradually become **information-hidden**. You can see the habitat exists but:

- Animal morale displays as "?" instead of a number
- Food delivery status is hidden
- Crisis events developing in that zone don't appear in your notification queue until you or an Acolyte enters the area

The darkness worsens at night — even recently-visited areas lose visibility faster without torches or lantern structures. An Acolyte on patrol duty (§5) maintains visibility for their assigned zone throughout the night.

This makes the keeper's physical movement genuinely valuable (walking near habitats is how you know they're okay), makes the night phase terrifying (half your zoo could be in cascade crisis and you can't see it from the other side of the map), and makes Acolyte patrol the most important late-game assignment.

*Connects to:* Avatar/keeper movement (existing — now has real information value beyond income collection), Day/Night cycle (§1), Acolytes (§5 — patrol duty is now the only way to maintain night visibility across the full zoo), Crisis Events (§8), Environmental Cascade Failures (§17 — cascades start invisible in dark zones).

---

#### 32. Wild Resource Camps — Jungle Pathing

The expedition gate and map edge formalized into a set of **Wild Camps** that respawn on known timers:

| Camp | Respawn | Yield |
|---|---|---|
| Berry Patch | Every 2 in-game days | Food materials (bulk; low value) |
| Ancient Burrow | Once per season | Random animal egg (species rolled from current season's pool) |
| Ley Line Node | Daily, night only | Raw DNA (1–3 units; keeper must physically walk to map edge) |
| Monster Den | Once per season | Triggers a Seasonal Megafauna Encounter (§25) |
| Overgrown Shrine | Rare random | Drops a Relic fragment (collect 3 fragments to assemble a full Relic) |

The keeper "jungles" — divides time between zoo management and camp harvesting. Camp locations are discovered by walking the map edge and stumbling on them, or revealed all at once by building the Observation Tower landmark (§28).

Acolytes can be assigned to harvest specific camps, freeing the keeper for zoo management — but an Acolyte at the dark map edge at night is a risky assignment. If a crisis fires inside the zoo while both the keeper and the Acolyte are at the edge, Fog of War means you won't know until someone returns.

*Connects to:* Expeditions (§11), Keeper movement (existing), Seasonal Megafauna Encounters (§25), Relics (§12 — fragments now have a gathering loop instead of dropping whole), DNA currency (existing), Fog of War (§31), Acolytes (§5), Observation Tower landmark (§28).

---

## Core World Architecture — The Procedural Wilderness & Catching Loop

> **This section supersedes §11 (Expeditions) and the shop-based acquisition model.** The shop is gone as the primary acquisition path. The Expedition abstraction is gone — "going on an expedition" is now just going out into the world. The Night Market / Wandering Merchant survives but is found in the wild as an NPC landmark rather than a timed popup. Everything else in the plan above layers onto this foundation.

---

### §33. The World Map

The game world is a large procedurally generated tile map. The player's zoo occupies a **fixed starting zone** at the center — it is part of the world, not a separate screen. The zoo can expand outward by claiming adjacent tiles (coins + materials to clear and claim each tile). Beyond the zoo boundary, wilderness stretches in all directions, initially fogged and revealed only by walking through it.

The world generates in **biome bands** corresponding to the existing seven habitat themes: Forest, Arctic, Savanna, Wetland, Jungle, Ocean, Farmland. Animals roam their natural biome. To find a Snow Lion you travel toward the Arctic band. To find aquatic species you find the Ocean band.

**Rarity gradient**: common species spawn close to the zoo. Rare and apex species require real distance. The world is never purely random — biomes have structure, rare species have territories, and certain landmarks only appear beyond a certain exploration radius.

A few **fixed NPCs** live at the zoo base permanently (§39). Everything else in the world — merchants, shrines, other keepers — is discovered through exploration.

---

### §34. The Catching Mechanic — Core Skill Loop

This is the primary acquisition path. Catching replaces buying.

1. Player enters **Catch Mode** (toggle or held key)
2. Hover cursor over a roaming wild animal — a **capture circle** appears centered on the animal
3. The circle fills as long as the cursor stays on the animal
4. The animal attempts to escape — it moves to break cursor contact
5. If the animal exits the circle's tracked radius, fill progress resets (partially or fully, per species)
6. Complete the fill → animal is caught and added to a carry inventory; bring it home to place in a habitat

**Difficulty knobs per species:**
- **Fill speed** — slow for rare species, fast for common ones
- **Move speed** — how aggressively it tries to escape
- **Reset behavior** — partial reset vs full reset when it breaks contact
- **Circle radius** — wider = more forgiving; rare species have tight circles
- **Stamina** — some animals exhaust themselves and slow down after sustained flight; apex species never tire

The mechanic is always readable — the circle is visible, progress is visible — but has a real skill ceiling from learning species-specific patterns.

---

### §35. Animal Movesets — Evasion Patterns

Each species has a defined evasion moveset (1–2 behaviors). Common species have one simple pattern. Rare and apex species combine two, sometimes in sequence. Learning the moveset is the mastery loop.

| Moveset | Behavior |
|---|---|
| **Zigzagger** | Sharp random direction changes at short intervals — unpredictable, but the changes telegraph slightly |
| **Burster** | Holds still briefly (lulling the player), then dashes in a straight line |
| **Circler** | Orbits the player at a fixed radius, always staying just outside easy reach |
| **Aggressor** | Moves *toward* the cursor — the player must hold their ground rather than chase |
| **Vanisher** | Disappears briefly and reappears several tiles away in a random direction |
| **Decoy** | Splits into visual duplicates; only one is real and catchable |
| **Freezer** | Moves very slowly but the circle fills extremely slowly — a patience test, not a reflex test |
| **Panicker** | Moves erratically at high speed but tires quickly; survive the first few seconds and it slows dramatically |

Example: Snow Lion (Apex) = **Burster + Aggressor**. It charges the player, forcing them to hold ground, then suddenly dashes away — the player has to pivot immediately and give chase before the circle resets.

---

### §36. Animal Mastery

Track cumulative catches per species across the whole save. Mastery increases by catching, not by owning — releasing or losing an animal never reduces it. This rewards players who understand and hunt the same species repeatedly.

**Mastery tiers (example thresholds: 1 / 5 / 15 / 40 / 100 catches):**

| Tier | Unlock |
|---|---|
| 0 | Default difficulty; moveset unknown |
| 1 | Moveset hint revealed in the bestiary ("tends to freeze before a burst") |
| 2 | Circle fill speed +10%; next-move direction briefly telegraphed with a subtle visual cue |
| 3 | Fill speed +20%; reset on break is now always partial, never full |
| 4 | Fill speed +30%; animal tires faster; rare behavioral event can trigger on catch (special animation, bonus material drop) |
| Mastered | Full bestiary entry illustrated; guaranteed partial reset on break; animal occasionally hesitates mid-flight |

Mastery is the MHW hunter knowledge loop: the first time you catch a species you're fumbling; by the hundredth catch you're reading its movements before they happen.

*Connects to:* Animal Movesets (§35), Species Research Dossiers (§22 — mastery = catching expertise, dossier = ownership expertise; they are complementary), Personality Traits (§10 — a Shy animal is always easier to catch regardless of mastery; an Aggressor-moveset animal with the Corrupted trait is genuinely threatening).

---

### §37. Wild Fog of War — Exploration

The procedural world outside the zoo starts fully fogged. Walking through tiles reveals them permanently — this is classic exploration fog, distinct from the zoo's information-hiding fog (§31) which resets if you stop visiting.

**Wild fog mechanics:**
- Fogged tiles show terrain silhouette only — no animals, no landmarks
- Animal sounds carry through fog: you can hear a species before you see it, cueing you to approach
- Some NPC landmarks emit a visible signal through nearby fog (smoke column, glow, music) — a lure drawing you toward discovery
- Mastered species leave faint tracks visible even in partially fogged adjacent tiles, rewarding experienced hunters with a head start

Exploration fog and zoo information fog are rendered differently so the player always knows which kind of darkness they're looking at.

*Connects to:* Animal Mastery (§36 — tracks visible in fog for mastered species), Zoo Fog of War (§31 — two distinct fog systems), Day/Night spawning (§40 — night exploration is darker and drains sanity, but nocturnal species only appear then).

---

### §38. NPC Landmarks in the Wild

Procedurally placed points of interest scattered through the wilderness. Frequency decreases with distance from base (common landmarks nearby, rarer ones further out). Discovered purely by exploration — no map, no markers until you find them.

| Landmark | Description |
|---|---|
| **Wandering Merchant's Camp** | Temporary — moves every few in-game days. Sells ritual components, relic fragments, rare eggs. The Night Market equivalent; found in the world rather than time-gated at base. |
| **Ancient Shrine** | Fixed. One-time interaction performs a ritual at no cost. Goes dormant after use. |
| **Ruined Observatory** | Fixed. Reveals a large radius of exploration fog on first visit. |
| **Trapper's Cache** | A hidden supply cache with food materials and occasionally a pre-caught animal someone left behind. |
| **Other Keeper's Camp** | NPC zookeeper who trades: give them a species they want, receive a rare one they have. |
| **Bone Circle** | Ritual site. Performing any ritual here doubles its effect but consumes the site permanently — one-time power. |
| **Megafauna Den** | The rare wandering animal from §25 now has a fixed territory in the world. You find the den, observe it, lure and capture rather than waiting for it to appear at your gate. |
| **Overgrown Shrine** | Drops a Relic fragment on first visit (replaces the random camp drop from §32). Collect 3 fragments to assemble a Relic. |

*Connects to:* Wild Fog of War (§37 — landmarks discovered through exploration), Relics (§12), Ritual System (§6), Seasonal Megafauna (§25 — den replaces gate encounter), Night Market (§13 — Wandering Merchant camp replaces timed popup).

---

### §39. Fixed Base NPCs

A small permanent cast at the zoo home base. These are the Research Base equivalent — you always return here between excursions into the wild. They provide services, react to what you bring back, and have dialogue arcs that unlock as your collection grows.

| NPC | Role |
|---|---|
| **The Vet** | Manages animal health and morale; can diagnose problems in habitats; unlocks new habitat care options through dialogue milestones |
| **The Architect** | Sells habitat structures, environment controls, base expansions; replaces the shop for infrastructure (coins, not animals) |
| **The Chronicler** | Maintains the Grimoire, bestiary, mastery records, and discovered synergies; reacts with genuine excitement when you bring in a species they haven't catalogued |
| **The Cook** | Processes raw food materials into compound feeds; runs the food recipe system (§19 Research Board, simplified) |
| **The Gatekeeper** | Manages the zoo entrance; tracks visitor Wonder and Patron conversions; recruits Acolytes on your behalf |

NPCs grow through your play: the Vet gets new dialogue when you bring in a species they've never treated; the Chronicler slowly reveals world lore as the Grimoire fills; the Cook unlocks new recipes as you discover new food material types in the wild.

---

### §40. Day/Night Spawning in the Wild

The day/night cycle (§1) now controls wilderness animal activity, not just zoo mechanics. The wilderness feels alive because it changes with the clock.

- **Dawn**: migration window — animals move between biome bands, high general activity, good time to observe movement patterns before committing to a catch
- **Day**: standard spawning; most species visible and catchable; safest time to explore new territory
- **Dusk**: nocturnal species begin emerging; diurnal species retreat into terrain; transitional species (crepuscular) appear only in this window
- **Night**: nocturnal species at peak activity; rare apex species more active and territorial; darkness makes navigation harder without a lantern structure or keeper lantern item; keeper sanity drains faster from the darkness (§2); some species **only ever spawn at night**

Seasonal modifiers stack on top: winter nights in the Arctic biome are the hardest conditions in the game — long night, extreme cold, dangerous apex species active — but that's where the rarest arctic catches are.

*Connects to:* Day/Night cycle (§1), Keeper Sanity (§2), Seasons (§9), Animal Movesets (§35 — some movesets are harder to read in low-light conditions), Wild Fog of War (§37 — night makes fog darker even in explored tiles without lanterns).

---

## What NOT to Add (anti-ideas)

- **Full permadeath** — the idle/tycoon identity would break. Consequences yes; losing the zoo no.
- **Combat** — nothing in the architecture or tone supports it. Escape events and turf wars are enough action.
- **Crafting trees deeper than 2 levels** — food → ritual component, material → equipment item. A deeper crafting system buries the zoo identity under production management.
- **NPC visitors as full simulation** — they're Wonder sources and patron seeds, not beings with full pathfinding. Keep them lightweight.
- **Actual fluid/gas physics** — Habitat Environment Parameters (§14) are abstracted per-habitat stats, not a tile-by-tile fluid sim. ONI's depth comes from its simulation layer; what we want are the player-facing decisions, not the underlying physics.
- **Combat in turf wars** — turf conflicts are notification events with morale consequences, not attack animations. The drama is ecological, not mechanical.
- **Full LoL-style banning in Zoo Expo** — Zoo Expo (§30) is a soft drafting moment. True ban phases and adversarial meta don't fit single-player zoo management.
- **Material crafting tree beyond 2 steps** — material → equipment or material → ritual component. Stop there.
- **Rebuilding the shop as a fallback** — catching IS the acquisition loop. If animals can always be bought as a backup, catching becomes optional and loses its meaning. The Wandering Merchant and Other Keeper NPC handle edge cases (trade-based acquisition) without restoring a purchase menu.
- **Purely random procedural generation** — the world needs biome structure and a rarity gradient (common near base, rare far out). Pure randomness produces a world with no sense of discovery or direction.
- **Instant catches even for mastered species** — mastery reduces difficulty, it never eliminates it. Every catch should have at least a moment of tension.

---

## Suggested Priority Order

**Layer 0 — World Foundation (must exist before anything else):**

1. **Procedural world map (§33)** — the game cannot be played without it. The zoo grid becomes a fixed zone in a larger generated world. All other systems assume this exists.
2. **The catching mechanic (§34)** — the core skill loop. Must feel good before anything else is built on top of it.
3. **Animal movesets (§35)** — even 2–3 movesets at launch make catches feel meaningfully different. The mechanic is shallow without them.

**Layer 1 — Foundation (unlock everything else):**

4. **Day/Night cycle (§1) + wild spawning (§40)** — time axis for all other systems; nocturnal species are the first reward for playing at night.
5. **Wild Fog of War (§37)** — makes exploration feel real; dead simple to implement (tiles start hidden, reveal on visit).
6. **Animal Morale (§3)** — makes food structures relevant, makes keeper movement purposeful, enables crises.
7. **Keeper Sanity + Hunger (§2)** — two floats + decay math; changes how the game *feels* immediately.

**Layer 2 — Home Base & Caregiving:**

8. **Fixed Base NPCs (§39)** — give the zoo a personality and make returning from the wild feel meaningful.
9. **Animal Mastery (§36)** — pure data tracking; immediately rewards repeated catching with visible progress.
10. **Zoo Fog of War (§31)** — information-hiding within the zoo; makes keeper patrol purposeful.
11. **Species Role Identity (§26) + Synergy Combos (§27)** — metadata on existing species; reframes the catalog as a composition game overnight.
12. **Food Routing (§15)** — requires rethinking structure/habitat interaction; unlocks Acolyte delivery routes as a spatial assignment.

**Layer 3 — Research & Long-term Progression:**

13. **Species Research Dossiers (§22)** — rewards long-term ownership; needs morale to be meaningful first.
14. **Animal Material Economy (§23)** — needs dossiers; provides alternative DNA path.
15. **Behavioral Ecology Tags (§20)** — adjacency decisions gain ecological weight; feeds turf wars and synergy combos.
16. **Acolyte Skill Specialization (§18)** — needs Acolytes with meaningful tasks first.

**Layer 4 — Wild Content & Endgame:**

17. **NPC Landmarks in the Wild (§38)** — needs the world map to exist and be worth exploring first.
18. **Megafauna Dens (§25 revised)** — needs field tracking and mastery system to make the encounter meaningful.
19. **Zoo Expo (§30)** — needs a full species roster and synergy system.
20. **Landmark Structures / Map Objectives (§28)** — needs a full zoo economy for real placement trade-offs.
21. **Research Board (§19)** — deepest infrastructure layer; ships last.

---

## Tone Notes

The visual/audio identity should lean into the tension. Don't Starve's world is beautiful *and* hostile. Cult of the Lamb is pastel *and* deeply weird. cmd_zoo should feel like: **a charming zoo run by someone who knows too much about what lives in the dark at the edge of the enclosures.** The animals glow softly at night. The keeper's shadow is the wrong shape when sanity is low. Hybrid species have names that sound almost normal until you look at them.
