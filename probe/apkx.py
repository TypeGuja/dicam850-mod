import zipfile, sys, os
apk, out = sys.argv[1], sys.argv[2]
z = zipfile.ZipFile(apk)
for n in z.namelist():
    if n in ('lib/x86_64/libicatch_wificam_sdk.so', 'lib/arm64-v8a/libicatch_wificam_sdk.so') or n.startswith('classes') or n.startswith('lib/arm64-v8a/lib') and 'Bugly' not in n:
        dst = os.path.join(out, n.replace('/', '_'))
        with open(dst, 'wb') as f: f.write(z.read(n))
        print('extracted', dst)
