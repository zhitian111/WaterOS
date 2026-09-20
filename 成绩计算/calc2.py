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
    w = 0.2 * (v.get("chu",0)/FULL["chu"]*100) + 0.2 * (v.get("online",0)/FULL["online"]*100) + 0.4 * (v.get("offline",0)/FULL["offline"]*100)
    rows.append({"team": team, "w": w})
rows.sort(key=lambda r: -r["w"])
rank = {r["team"]: i+1 for i, r in enumerate(rows)}

os_row = [r for r in rows if r["team"].startswith("Outer")][0]
os_rank = rank[os_row["team"]]
os_w = os_row["w"]
print(f"OuterSystems 排名: {os_rank} / {len(rows)}，前三阶段加权分 w = {os_w:.4f}")
print(f"前24名截止（第24名）: {rows[23]['team']} w={rows[23]['w']:.4f}")
print(f"第24名与OuterSystems差距 = {os_w - rows[23]['w']:.4f}")
print(f"当前在OuterSystems前面的人数 = {os_rank - 1}")

# 若大家答辩同分，名次不变。这里假设“进前24”需要排名<=24。
print(f"\n[假设1] 全员答辩同分 -> OuterSystems 第{os_rank}名，已在前24名内，无需追赶任何人。")
print(f"[假设1] 安全垫：后面有 {len(rows) - os_rank} 支队伍，最多可掉到第24名，即允许 {24 - os_rank} 支后面的队伍反超仍在前24。")

# 场景2: 答辩满分100，看后面队伍需要比OuterSystems高多少答辩分才能反超进前24
# 反超条件: 0.2*O_d + w_o > 0.2*B_d + w_b  =>  B_d - O_d > (w_o - w_b)/0.2
print("\n[场景2] 答辩分数未知 -> 分析后面队伍要反超需要高出的答辩分(百分制):")
behind = rows[os_rank:]  # 排名在后面的
need = []
for r in behind:
    gap = (os_w - r["w"]) / 0.2
    need.append((r["team"], r["w"], gap))
# 前几名需要最小差距的（威胁最大的）
need.sort(key=lambda x: x[2])
print("威胁最大的10支队伍（它们答辩需比OuterSystems高出的分数）:")
for team, w, g in need[:10]:
    print(f"  {team:20s} w={w:6.3f}  需高 {g:6.2f} 分(答辩)")
print(f"\n最小需求（最接近的追赶者）：{need[0][0]} 需答辩比OuterSystems高 {need[0][2]:.2f} 分才能反超")

# 场景3: 要掉出前24，需要被24-16=8支后面的队伍反超；看第8名威胁者
if len(need) >= 8:
    t8 = need[7]
    print(f"\n[场景3] 若要掉出前24（被8支反超）：第8个威胁者 {t8[0]} 需答辩比OS高 {t8[2]:.2f} 分")
    # 最坏情况：后面7支答辩100，OS答辩X，仍第24名需要的X
    worst = []
    for team, w, g in need[:7]:
        # OS答辩x, 该队答辩100: 反超条件 x + gap < 100 => x < 100 - gap
        worst.append(100 - g)
    # 要保持第24名(>=第24)，只需不被第8个(need[7])超过；OS答辩需>= 100 - need[7].gap? 不对，那是排名第24的最后一个位置
    print(f"  最坏情况下(后7名答辩100分) 要保持前24，OS答辩需 >= {100 - t8[2]:.2f} 分")

# 场景4: OS答辩X分，其他全部答辩满分100时，OS会掉到多少名（看最多被多少人反超）
os_x = 100.0
cnt = sum(1 for team, w, g in need if w + 0.2*100 > os_w + 0.2*os_x)
print(f"\n[场景4] 若OS答辩{os_x}分且后面所有队答辩都100分: OS被{cnt}支反超，落到第{os_rank+cnt}名")
os_x = 90.0
cnt = sum(1 for team, w, g in need if w + 0.2*100 > os_w + 0.2*os_x)
print(f"[场景4] 若OS答辩{os_x}分且后面所有队答辩都100分: OS被{cnt}支反超，落到第{os_rank+cnt}名")
os_x = 80.0
cnt = sum(1 for team, w, g in need if w + 0.2*100 > os_w + 0.2*os_x)
print(f"[场景4] 若OS答辩{os_x}分且后面所有队答辩都100分: OS被{cnt}支反超，落到第{os_rank+cnt}名")
os_x = 70.0
cnt = sum(1 for team, w, g in need if w + 0.2*100 > os_w + 0.2*os_x)
print(f"[场景4] 若OS答辩{os_x}分且后面所有队答辩都100分: OS被{cnt}支反超，落到第{os_rank+cnt}名")
