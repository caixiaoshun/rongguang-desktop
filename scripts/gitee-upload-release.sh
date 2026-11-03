#!/usr/bin/env bash
# 把已构建好的安装包补传到 Gitee Release（国内下载镜像）。
#
# 为什么不在 CI 里传：GitHub runner 到 Gitee 单连接约 0.2MB/s，三个包传不完，
# 一次实跑卡死 45 分钟只能取消。附件统一由维护者本机补传。
#
# 用法：
#   GITEE_TOKEN=xxxxx ./scripts/gitee-upload-release.sh v0.1.1 dist/
#
# dist/ 里放安装包（文件名保持 xx_x.y.z_x64-setup.exe 这种原名）。
set -euo pipefail

TAG="${1:?用法: GITEE_TOKEN=xxx $0 <tag> <产物目录>}"
DIR="${2:?缺少产物目录}"
: "${GITEE_TOKEN:?需要环境变量 GITEE_TOKEN}"

OWNER="caixiaoshun"
REPO="ai-fusion-video"
API="https://gitee.com/api/v5/repos/${OWNER}/${REPO}"

[ -d "$DIR" ] || { echo "目录不存在: $DIR"; exit 1; }

# 1. 找到 release（Gitee 对不存在的 tag 返回 200 + null，只能看能不能取到 id）
REL_ID=$(curl -sS "${API}/releases/tags/${TAG}?access_token=${GITEE_TOKEN}" \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('id','') if isinstance(d,dict) else '')")
[ -n "$REL_ID" ] || { echo "Gitee 上不存在 release ${TAG}，请先跑一次 CI 建好容器"; exit 1; }
echo "release id = ${REL_ID}"

# 2. 删掉同名旧附件（Gitee 重复上传会堆记录，不覆盖）
NAMES=$(cd "$DIR" && ls | grep -E '\.(exe|deb|dmg)$' || true)
[ -n "$NAMES" ] || { echo "目录里没有 exe/deb/dmg"; exit 1; }

EXIST=$(curl -sS "${API}/releases/${REL_ID}/attach_files?access_token=${GITEE_TOKEN}")
DEL_IDS=$(printf '%s' "$EXIST" | NAMES="$NAMES" python3 -c "
import sys,json,os
try: items=json.load(sys.stdin) or []
except Exception: items=[]
names={n for n in os.environ['NAMES'].splitlines() if n}
for a in items:
    if a.get('name') in names: print(a['id'])
")
for aid in $DEL_IDS; do
  echo "删除同名旧附件 id=${aid}"
  curl -sS -o /dev/null -X DELETE \
    "${API}/releases/${REL_ID}/attach_files/${aid}?access_token=${GITEE_TOKEN}" || true
done

# 3. 串行上传（本机带宽够，串行避免 Gitee 侧并发限流）
for n in $NAMES; do
  f="$DIR/$n"
  SIZE=$(stat -c%s "$f" 2>/dev/null || stat -f%z "$f")
  if [ "$SIZE" -gt 104857600 ]; then
    echo "SKIP ${n}（$(echo "$SIZE" | awk '{printf "%.1f", $1/1048576}')MB 超过 Gitee 单附件 100MB 上限）"
    continue
  fi
  echo -n "上传 ${n} ... "
  CODE=$(curl -sS -o "/tmp/gitee_${n}.json" -w '%{http_code}' \
    --retry 3 --retry-delay 5 -X POST \
    "${API}/releases/${REL_ID}/attach_files" \
    -F "access_token=${GITEE_TOKEN}" -F "file=@${f}")
  if [ "$CODE" = "201" ] || [ "$CODE" = "200" ]; then echo "OK"; else echo "FAIL HTTP ${CODE}"; fi
done

# 4. 回读核对
echo "=== Gitee 附件清单 ==="
curl -sS "${API}/releases/${REL_ID}/attach_files?access_token=${GITEE_TOKEN}" \
  | python3 -c "
import sys,json
for a in json.load(sys.stdin) or []:
    print(' ', a['name'], round(a.get('size',0)/1048576,1), 'MB')
"
