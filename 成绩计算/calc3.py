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

rows = []
for team, v in d.items():
    w = 0.2*(v.get("chu",0)/FULL["chu"]*100) + 0.2*(v.get("online",0)/FULL["online"]*100) + 0.4*(v.get("offline",0)/FULL["offline"]*100)
    rows.append({"team": team, "w": w})
rows.sort(key=lambda r: -r["w"])
teams = [r["team"] for r in rows]
N = len(teams)
OS = "OuterSystems"
os_idx = teams.index(OS)  # 0-based，第 os_idx+1 名
os_w = rows[os_idx]["w"]

def defense_score(rank, spread_min, spread_max, n):
    """rank 1 = 最好(=最高分), rank n = 最差。线性分布在 [spread_min, spread_max]"""
    if n <= 1:
        return spread_max
    return spread_max - (rank-1) * (spread_max - spread_min) / (n - 1)

def worst_case_rank(R_os, spread_min, spread_max):
    """对抗性最坏情况：OS答辩排第R_os名；
       排名在OS后面的队伍拿最好的答辩名次(1..)，前面的队伍拿最差的答辩名次。
       返回OS最终总排名。"""
    # 构建每支队伍答辩分数（对抗性）
    behind = teams[os_idx+1:]  # 后面(104支)
    ahead = teams[:os_idx]     # 前面(15支)
    # 后面的拿最好名次 1..len(behind)
    # OS 拿 R_os
    # 前面的拿最差名次
    dscore = {}
    for i, t in enumerate(behind, start=1):
        dscore[t] = defense_score(i, spread_min, spread_max, N)
    dscore[OS] = defense_score(R_os, spread_min, spread_max, N)
    start_worst = max(len(behind), 1)
    for j, t in enumerate(ahead, start=1):
        dscore[t] = defense_score(N - j + 1, spread_min, spread_max, N)
    # 计算总分并排名
    totals = []
    for t in teams:
        w = rows[teams.index(t)]["w"]
        totals.append((t, w + 0.2 * dscore[t]))
    totals.sort(key=lambda x: -x[1])
    return [t for t, _ in totals].index(OS) + 1

for name, (smin, smax) in [("均匀[0,100]", (0,100)), ("均匀[60,95]", (60,95)), ("均匀[70,100]", (70,100))]:
    print(f"\n=== 答辩分分布: {name} ===")
    safe = []
    for R in range(1, N+1):
        fr = worst_case_rank(R, smin, smax)
        if fr <= 24:
            safe.append(R)
    if safe:
        print(f"  最坏情况下仍能进前24的答辩名次范围: 1 ~ {safe[-1]} (即答辩排前 {safe[-1]} 名都稳)")
    else:
        print("  无安全名次")
    # 几个示例
    for R in [16, 30, 40, 60, 80, 100, 110, 120]:
        fr = worst_case_rank(R, smin, smax)
        print(f"  OS答辩排第{R}名 -> 最坏情况总排名第{fr}名")
