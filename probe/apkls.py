import zipfile, sys
for p in sys.argv[1:]:
    z = zipfile.ZipFile(p)
    names = z.namelist()
    print('==', p, len(names), 'entries')
    for n in names:
        if n.endswith(('.so', '.apk', '.xapk')) or n.startswith('classes') or n == 'manifest.json':
            print(f'  {z.getinfo(n).file_size:>10} {n}')
