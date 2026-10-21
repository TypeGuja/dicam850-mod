import sys, re, struct
f=open(sys.argv[1],'rb').read()
print("size",len(f))
# collect ascii runs with offsets
runs=[(m.start(),m.group().decode('latin1')) for m in re.finditer(rb'[\x20-\x7e]{4,}',f)]
# section-like tags: known iCatch section names
tags=['SUNP BURN FILE','DRAM CODE','BOOT','ICATCH','SPCA','V35','sensor','SENSOR','AUTOEXEC','autoexec','.ash','ISP','ISP_','AE_','AWB','AEC','AGC','3A','GAIN','EXPO','exposure','Exposure','LUMA','luma','.ini','.INI','.bin','.BIN','Table','TABLE','FW_VER','VERSION','Ver=','imx','IMX','GC','SP_','param','PARAM','tuning','Tuning','.txt']
import collections
seen=collections.OrderedDict()
for off,s in runs:
    for t in tags:
        if t in s:
            key=(t)
            seen.setdefault(t,[]).append((off,s))
for t,lst in seen.items():
    print(f"\n### tag {t!r}: {len(lst)} hits (first 6)")
    for off,s in lst[:6]:
        print(f"  @{off:#010x} {s[:90]!r}")
