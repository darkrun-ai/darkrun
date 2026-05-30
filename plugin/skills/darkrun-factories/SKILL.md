---
name: darkrun-factories
description: List available darkrun factories and their Stations
---

# Factories

## Process

1. **Call `darkrun_factory_list`** for the registered factories with descriptions and their Station
   sequences.

2. **Present them** as a short table: factory → what it's for → Stations. For the **software**
   factory, the Stations are `Frame → Specify → Shape → Build → Prove → Harden`, ordered by
   cost-of-late-discovery — each kills one class of rework and locks one durable artifact.

3. If the user is choosing where to run work, recommend the closest-fit factory and offer
   `/darkrun:darkrun-start`.
