import re

base = "/Users/x/code/WaterOS/成绩计算/"

def parse_table(fname):
    with open(base + fname, encoding="utf-8") as f:
        lines = [l for l in f.read().splitlines() if l.strip().startswith("|")]
    data = []
    for l in lines:
        cells = [c.strip() for c in l.strip().strip("|").split("|")]
        if all(re.fullmatch(r":?-{2,}:?", c) for c in cells if c != ""):
            continue
        data.append(cells)
    out = []
    for cells in data:
        if len(cells) < 4:
            continue
        try:
            out.append((cells[2], float(cells[-1])))
        except Exception:
            continue
    return out

p = parse_table("初赛.md"); o = parse_table("决赛线上.md"); l = parse_table("决赛线下.md")
FULL = {"chu": 2900.0, "online": 758.2, "offline": 1320.0}
d = {}
for lst, k in [(p, "chu"), (o, "online"), (l, "offline")]:
    for team, s in lst:
        d.setdefault(team, {})[k] = s

# 决赛线上49队为总池
finalists = [t for t, _ in o]
N = len(finalists)

def rank_in(teams_subset, stage_key):
    """在给定队伍集合里，按该阶段原始分排名(高分在前)"""
    scored = [(t, d[t].get(stage_key, 0.0)) for t in teams_subset]
    scored.sort(key=lambda x: -x[1])
    return {t: i+1 for i, (t, s) in enumerate(scored)}

r_chu = rank_in(finalists, "chu")
r_on  = rank_in(finalists, "online")
r_off = rank_in(finalists, "offline")

print(f"总池: 决赛线上{N}支队伍\n")
print("OS 在各阶段(仅49队内)的名次与百分制:")
for k, r in [("chu", r_chu), ("online", r_on), ("offline", r_off)]:
    print(f"  {k}: 原始分 {d['OuterSystems'].get(k,0):9.2f} / 满分 {FULL[k]:7.1f} -> 百分制 {d['OuterSystems'].get(k,0)/FULL[k]*100:6.2f} , 49队内第 {r['OuterSystems']} 名")

# 展示：初赛全121队名次 vs 49队内名次
all_chu = sorted([(t, s) for t, s in p], key=lambda x: -x[1])
r_chu_all = {t: i+1 for i, (t, s) in enumerate(all_chu)}
print(f"\n初赛在全部121队中的名次: 第 {r_chu_all['OuterSystems']} 名 ; 在49队内: 第 {r_chu['OuterSystems']} 名")

# 对比：综合排16名，但单项名次都不高，看看原因 —— 其他强队单项很高但其他项塌方
print("\n== 综合排名 vs 单项排名对比（前20名 + OS附近） ==")
rows = []
for t in finalists:
    v = d[t]
    w = 0.2*(v.get("chu",0)/FULL["chu"]*100) + 0.2*(v.get("online",0)/FULL["online"]*100) + 0.4*(v.get("offline",0)/FULL["offline"]*100)
    rows.append((t, w, v.get("chu",0)/FULL["chu"]*100, v.get("online",0)/FULL["online"]*100, v.get("offline",0)/FULL["offline"]*100))
rows.sort(key=lambda x: -x[1])
print(f"{'综合名次':<6}{'队伍':<20}{'综合':>7} {'初赛(20%)':>10} {'线上(20%)':>10} {'线下(40%)':>10}  单项名次(初/线/下)")
for i, (t, w, c, oo, ff) in enumerate(rows[:28], 1):
    m = " <== OS" if t.startswith("Outer") else ""
    print(f"{i:<6}{t:<20}{w:>7.2f} {c:>10.2f} {oo:>10.2f} {ff:>10.2f}  {r_chu[t]}/{r_on[t]}/{r_off[t]}{m}")
