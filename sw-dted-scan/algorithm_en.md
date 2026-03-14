# Terrain Data Compression & Query Algorithm

## Efficient Terrain System Based on Residual Grid & Base62 Encoding

---

## Overview

### Problem

Raw terrain point cloud data (DTED, LiDAR) contains hundreds of thousands to millions of 3D points (x, y, z):
- **Large size**: Raw CSV format occupies MBs to GBs
- **Query need**: Fast interpolation of height y from (x, z) coordinates
- **Storage efficiency**: Compression required for storage and transmission

### Solution

**Two-level compression**: Residual Grid + Base62 Encoding

```
Point Cloud → Grid → Residuals → Base62 → Compressed Data
                 ↓
           Base Height + Residual Correction
```

### Key Advantages

| Feature | Value |
|---------|-------|
| **Compression Rate** | 31.4% (68.6% space saved) |
| **Query Speed** | 45,000 queries/sec |
| **Accuracy** | Avg error < 1m |
| **Dependencies** | Zero (pure Lua) |
| **Modes** | File I/O + Embedded data |

---

## System Flow

### Compression Pipeline

```
┌─────────────────┐
│  Input: CSV     │
│  (x,y,z) × N    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  1. Grid Setup  │
│  R = 20m res    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  2. Base Height │
│  h_avg per grid │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  3. Residuals   │
│  δ = y - h_avg  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  4. Threshold   │
│  |δ| >= 1.0m ?  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  5. Base62 Enc  │
│  Int → Text     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Output Files   │
└─────────────────┘
```

### Query Pipeline

```
┌─────────────────┐
│  Input (x, z)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  1. Grid Index  │
│  gx, gz         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  2. LRU Cache   │
│  Hit? → Return  │
└────────┬────────┘
         │ Miss
         ▼
┌─────────────────┐
│  3. Base Height │
│  h[gx][gz]      │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  4. Residual IDW│
│  Gaussian Weight│
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  y = h + δ      │
│  Cache & Return │
└─────────────────┘
```

---

## Core Algorithms

### Grid Partitioning

```python
gx = floor((x - min_x) / RESOLUTION)
gz = floor((z - min_z) / RESOLUTION)

h_base[gx][gz] = mean(all y in grid[gx][gz])
```

### Residual Calculation

```python
residual = y_actual - h_base[gx][gz]

# Store only significant residuals
if abs(residual) >= THRESHOLD:
    encode_and_store()
```

### Base62 Encoding

```
Charset: 0-9 A-Z a-z (62 chars)

Examples:
  0    → "0"
  61   → "z"
  62   → "10"
  -5   → "-5"

Saves ~30% vs decimal text
```

### Query: Inverse Distance Weighting

```lua
-- Gaussian weight (avoids sqrt)
weight = exp(-dist² / radius²)

residual = Σ(r_i × w_i) / Σ(w_i)

height = base_height + residual
```

---

## Data Format

### Compressed File (terrain_compressed.dat)

```
#GRID
gx,gz,height_b62
0,37,2bu
5,1b,2bn
...

#RESIDUALS
gx,gz,x_off,z_off,residual_b62
L,3H,2cc,Fb,m,In
...
```

### Metadata (terrain_meta.json)

```json
{
  "min_x": -9128.71,
  "min_z": -12990.15,
  "resolution": 20.0,
  "residual_threshold": 1.0,
  "version": 1
}
```

---

## Performance Results

### Test Dataset

| Metric | Value |
|--------|-------|
| Input points | 389,636 |
| Original size | 8,375 KB |
| Terrain type | Mixed (flat + hills) |

### Compression Results (20m/1m)

| Metric | Value |
|--------|-------|
| Grid cells | 201,094 |
| Significant residuals | 63,709 (16.4%) |
| Compressed size | 2,632 KB |
| **Compression rate** | **31.4%** |

### Query Performance

| Metric | Value |
|--------|-------|
| Single query | 0.022 ms |
| Throughput | 45,000/sec |
| Batch query | 27,000/sec |
| Cache hit (local) | >90% |
| Cache hit (random) | ~3% |

### Accuracy Analysis

| Parameters | Avg Error | Max Error | RMS Error |
|------------|-----------|-----------|-----------|
| 10m / 0.5m | 0.14 m | 1.10 m | 0.37 m |
| **20m / 1.0m** | **0.66 m** | **8.96 m** | **0.81 m** |
| 50m / 2.0m | 3.04 m | 18.90 m | 1.74 m |

### Complexity

| Operation | Time | Space |
|-----------|------|-------|
| Grid lookup | O(1) | O(grid_cells) |
| Residual interpolation | O(n), n<10 | O(residuals) |
| **Total query** | **O(1)** | **~5MB RAM** |

---

## Usage

### Compression (Python)

```python
# compress_terrain.py
RESOLUTION = 20.0        # meters
RESIDUAL_THRESHOLD = 1.0 # meters

python3 compress_terrain.py
# Output: terrain_compressed.dat, terrain_meta.json
```

### Query - File Mode (Lua)

```lua
local TQ = require("terrain_query")
local tq = TQ.new("terrain_compressed.dat", "terrain_meta.json")

local y = tq:h(x, z)           -- Single query
local heights = tq:hb(coords)  -- Batch query
local stats = tq:cs()          -- Cache stats
```

### Query - Embedded Mode (Lua)

```lua
local TD = require("terrain_data")  -- Embedded data
local TQ = require("terrain_query")
local meta = {min_x=-9128, min_z=-12990, resolution=20}

local tq = TQ.new(TD, meta)
local y = tq:h(x, z)
```

### Stormworks Integration

```lua
-- In mod.lua or script
local terrain = nil

function tick()
    if not terrain then
        terrain = require("terrain_query").new(
            "terrain_compressed.dat",
            "terrain_meta.json"
        )
    end
    
    local x, z = get_player_pos()
    local y = terrain:h(x, z)
    
    -- Use height value...
end
```

---

## Configuration Guide

| Scenario | Resolution | Threshold | Size | Error |
|----------|------------|-----------|------|-------|
| Flat terrain | 50m | 2.0m | 25% | ~3m |
| **General** | **20m** | **1.0m** | **31%** | **~0.7m** |
| Complex hills | 10m | 0.5m | 37% | ~0.1m |
| High precision | 5m | 0.3m | 45% | <0.1m |

---

## File Structure

```
dted/
├── compress_terrain.py    # Python compressor
├── terrain_query.lua      # Lua query module
├── terrain_compressed.dat # Compressed data
├── terrain_meta.json      # Metadata
├── terrain_data.lua       # Embedded data (optional)
├── benchmark.lua          # Performance test
└── verify_accuracy.lua    # Accuracy validation
```

---

## API Reference

### terrain_query.lua

| Method | Parameters | Returns | Description |
|--------|------------|---------|-------------|
| `T.new()` | dat_file, meta_file | TQ instance | File mode |
| `T.new()` | data_str, meta_tbl | TQ instance | Embedded mode |
| `tq:h()` | x, z | y | Query height |
| `tq:hb()` | {{x,z},...} | {y,...} | Batch query |
| `tq:cs()` | - | {e,h,m,r,s} | Cache stats |
| `tq:cc()` | - | - | Clear cache |

---

## Summary

**What we built:**
- Terrain compression system with 68.6% space reduction
- Fast query engine (45K queries/sec)
- Sub-meter accuracy (0.66m average error)
- Zero external dependencies
- Dual-mode deployment (file/embedded)

**Best for:**
- Game terrain systems (Stormworks, etc.)
- GIS applications
- Embedded systems with limited storage
- Real-time terrain queries

---

*Version 1.0 | 2026-03-13*
