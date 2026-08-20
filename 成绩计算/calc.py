import re

base = "/Users/x/code/WaterOS/成绩计算/"

def parse_table(fname, score_col=-1):
    rows = []
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
            team = cells[2]
            score = float(cells[-1])
        except Exception:
            continue
        out.append({"team": team, "score": score})
    return out

p = parse_table("初赛.md")     # 满分 2900
o = parse_table("决赛线上.md") # 满分 758.2
l = parse_table("决赛线下.md") # 满分 1320

print("初赛 rows:", len(p), "线上 rows:", len(o), "线下 rows:", len(l))
for name, lst in [("初赛", p), ("线上", o), ("线下", l)]:
    for r in lst:
        if r["team"].startswith("Outer"):
            print("OuterSystems", name, r)

# 合并
d = {}
for lst, key in [(p, "chu"), (o, "online"), (l, "offline")]:
    for r in lst:
        d.setdefault(r["team"], {})[key] = r["score"]

FULL = {"chu": 2900.0, "online": 758.2, "offline": 1320.0}

# 加权平均（不含答辩 20%）: 0.2*初赛 + 0.2*线上 + 0.4*线下  归一化到100分
rows = []
for team, v in d.items():
    def norm(k):
        return v.get(k, 0.0) / FULL[k] * 100.0
    weighted = 0.2 * norm("chu") + 0.2 * norm("online") + 0.4 * norm("offline")
    rows.append({
        "team": team,
        "chu": v.get("chu", 0.0),
        "online": v.get("online", 0.0),
        "offline": v.get("offline", 0.0),
        "w": weighted,
    })

rows.sort(key=lambda r: -r["w"])
print("\n=== 前三项加权排名（不含答辩）===")
for i, r in enumerate(rows, 1):
    mark = " <== OuterSystems" if r["team"].startswith("Outer") else ""
    print(f"{i:3d} {r['team']:24s} w={r['w']:7.3f}  初赛={r['chu']:8.1f} 线上={r['online']:7.1f} 线下={r['offline']:7.1f}{mark}")

os_rank = [i for i, r in enumerate(rows, 1) if r["team"].startswith("Outer")][0]
print("\nOuterSystems 前三项加权排名:", os_rank, "/", len(rows))
