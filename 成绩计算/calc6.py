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

finalists = [t for t, _ in o]
N = len(finalists)
rows = []
for t in finalists:
    v = d[t]
    w = 0.2*(v.get("chu",0)/FULL["chu"]*100) + 0.2*(v.get("online",0)/FULL["online"]*100) + 0.4*(v.get("offline",0)/FULL["offline"]*100)
    rows.append({"team": t, "w": w})
rows.sort(key=lambda r: -r["w"])
teams = [r["team"] for r in rows]
OS = "OuterSystems"
os_idx = teams.index(OS)

def defense_score(rank, smin, smax, n):
    if n <= 1: return smax
    return smax - (rank-1)*(smax-smin)/(n-1)

def final_rank(dscore):
    totals = [(t, rows[teams.index(t)]["w"] + 0.2*dscore[t]) for t in teams]
    totals.sort(key=lambda x: -x[1])
    return [t for t, _ in totals].index(OS)+1

# 场景1: 对抗最坏（OS答辩第10名，后面队伍拿最好答辩，前面拿最差）
def worst(R_os, smin, smax):
    behind, ahead = teams[os_idx+1:], teams[:os_idx]
    ds = {}
    for i,t in enumerate(behind,1): ds[t]=defense_score(i,smin,smax,N)
    ds[OS]=defense_score(R_os,smin,smax,N)
    for j,t in enumerate(ahead,1): ds[t]=defense_score(N-j+1,smin,smax,N)
    return final_rank(ds)

print(f"总池: {N} 队，OS 三阶段加权第 {os_idx+1} 名 (w={rows[os_idx]['w']:.2f})")
print(f"\n【场景1 对抗最坏】OS答辩第10名（9队答辩比它高，其余都比它低）:")
for name,(a,b) in [("均匀[0,100]",(0,100)),("均匀[60,95]",(60,95)),("均匀[70,100]",(70,100))]:
    print(f"  {name}: 综合第 {worst(10,a,b)} 名")

# 场景2: 中性 —— 答辩分数与三阶段实力大致正相关（大家都按自己名次给答辩分）
#   OS答辩第10名 => OS答辩分 = 第10名的分数；其他队答辩分 = 按其综合名次给
print(f"\n【场景2 中性】答辩分按综合名次线性分布，OS拿第10名的分:")
for name,(a,b) in [("均匀[0,100]",(0,100)),("均匀[60,95]",(60,95)),("均匀[70,100]",(70,100))]:
    ds={}
    for i,t in enumerate(teams,1):
        ds[t]=defense_score(i,a,b,N)
    ds[OS]=defense_score(10,a,b,N)
    print(f"  {name}: 综合第 {final_rank(ds)} 名")

# 场景3: 简单假设 —— 答辩大家普遍同分(只OS略高，排第10)
print(f"\n【场景3】答辩同分基准 + OS是第10名:")
# 其他队答辩分 = 基准，OS = 基准 + (第10名 vs 中间) 差距；简化：OS答辩100，其他队按名次给分
for name,(a,b) in [("均匀[0,100]",(0,100)),("均匀[60,95]",(60,95)),("均匀[70,100]",(70,100))]:
    ds={}
    for i,t in enumerate(teams,1):
        ds[t]=defense_score(i,a,b,N)
    ds[OS]=b  # 最高分
    print(f"  {name} (OS满分): 综合第 {final_rank(ds)} 名")

# 也看看答辩第5名/第3名
print(f"\n【补充】对抗最坏下 OS答辩名次 -> 综合名次:")
for R in [3,5,8,10,12,15]:
    frs=[worst(R,a,b) for _,a,b in [("",0,100),("",60,95),("",70,100)]]
    print(f"  答辩第{R}名 -> 综合第 {frs[0]}/{frs[1]}/{frs[2]} 名 (三种分布)")
