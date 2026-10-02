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

# 只保留决赛线上(49支)的队伍作为总池
online_teams = [t for t, _ in o]
rows = []
for team in online_teams:
    v = d[team]
    w = 0.2*(v.get("chu",0)/FULL["chu"]*100) + 0.2*(v.get("online",0)/FULL["online"]*100) + 0.4*(v.get("offline",0)/FULL["offline"]*100)
    rows.append({"team": team, "w": w})
rows.sort(key=lambda r: -r["w"])
teams = [r["team"] for r in rows]
N = len(teams)
print(f"总队伍数(按决赛线上): {N}")

OS = "OuterSystems"
os_idx = teams.index(OS)
os_w = rows[os_idx]["w"]
print(f"\nOuterSystems 排名: {os_idx+1} / {N}，加权分 w={os_w:.4f}")
print(f"前24名截止(第24名): {rows[23]['team']} w={rows[23]['w']:.4f}")
print(f"领先第24名 = {os_w - rows[23]['w']:.4f} 加权分 = {(os_w - rows[23]['w'])/0.2:.2f} 答辩分")
print(f"前面人数 = {os_idx}，安全垫：可掉 {24 - (os_idx+1)} 名仍在前24")
print(f"\n完整排名:")
for i, r in enumerate(rows, 1):
    m = " <== OS" if r["team"].startswith("Outer") else ""
    print(f"{i:3d} {r['team']:22s} w={r['w']:7.3f}{m}")

def defense_score(rank, smin, smax, n):
    if n <= 1:
        return smax
    return smax - (rank-1)*(smax-smin)/(n-1)

def worst_case_rank(R_os, smin, smax):
    behind = teams[os_idx+1:]
    ahead = teams[:os_idx]
    dscore = {}
    for i, t in enumerate(behind, start=1):
        dscore[t] = defense_score(i, smin, smax, N)
    dscore[OS] = defense_score(R_os, smin, smax, N)
    for j, t in enumerate(ahead, start=1):
        dscore[t] = defense_score(N-j+1, smin, smax, N)
    totals = []
    for t in teams:
        totals.append((t, rows[teams.index(t)]["w"] + 0.2*dscore[t]))
    totals.sort(key=lambda x: -x[1])
    return [t for t, _ in totals].index(OS)+1

print(f"\n=== 答辩名次对抗性模拟 (N={N}) ===")
for name, (smin, smax) in [("均匀[0,100]",(0,100)), ("均匀[60,95]",(60,95)), ("均匀[70,100]",(70,100))]:
    safe = [R for R in range(1, N+1) if worst_case_rank(R, smin, smax) <= 24]
    print(f"\n[{name}] 稳进前24的答辩名次上限: 前 {safe[-1] if safe else '无'} 名")
    for R in [16, 20, 25, 30, 35, 40, 45, 49]:
        print(f"  答辩第{R}名 -> 最坏总排名第{worst_case_rank(R, smin, smax)}名")
