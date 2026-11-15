"""Regenerate the committed, renamed OFL fixtures with FontTools 4.68.0.

Run: python tools/generate_test_fonts.py
Downloads/cache and maintenance tooling live only under ignored target/m4-fonts.
Ordinary tests need neither Python, FontTools nor network access.
"""
import hashlib
import io
import json
from pathlib import Path
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / 'target/m4-fonts'
sys.path.insert(0, str(CACHE / 'tooling'))
import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont
assert fontTools.__version__ == '4.68.0', fontTools.__version__
DEST = ROOT / 'tests/fixtures/fonts'
DEST.mkdir(parents=True, exist_ok=True)
SOURCES = [
    ('latin', '2.015', 'https://github.com/notofonts/latin-greek-cyrillic/releases/download/NotoSans-v2.015/NotoSans-v2.015.zip', 'NotoSans/unhinted/ttf/NotoSans-Regular.ttf', 'Geello Test Sans', ''.join(chr(i) for i in range(32, 127)) + 'éèöÅáñ\u0301'),
    ('arabic', '2.021', 'https://github.com/notofonts/arabic/releases/download/NotoNaskhArabic-v2.021/NotoNaskhArabic-v2.021.zip', 'NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Regular.ttf', 'Geello Test Arabic', ''.join(chr(i) for i in range(0x600, 0x700)) + ' '),
    ('indic', '2.007', 'https://github.com/notofonts/devanagari/releases/download/NotoSansDevanagari-v2.007/NotoSansDevanagari-v2.007.zip', 'NotoSansDevanagari/unhinted/ttf/NotoSansDevanagari-Regular.ttf', 'Geello Test Devanagari', ''.join(chr(i) for i in range(0x900, 0x980)) + ' '),
    ('cjk', '2.004', 'https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf', None, 'Geello Test CJK', '中国地图北京上海你好世界测试字体标签中文 '),
]

def fetch(url, path):
    if not path.exists():
        with urllib.request.urlopen(url, timeout=120) as response:
            path.write_bytes(response.read())
    return path.read_bytes()

manifest = {'tool': 'FontTools 4.68.0 (MIT; maintenance only)', 'fonts': []}
for key, version, url, member, family, text in SOURCES:
    archive = fetch(url, CACHE / (key + ('.zip' if member else '.otf')))
    if member:
        with zipfile.ZipFile(io.BytesIO(archive)) as package:
            data = package.read(member)
            license_data = package.read('OFL.txt')
    else:
        data = archive
        license_data = fetch('https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/LICENSE', CACHE / 'cjk-OFL.txt')
    source = TTFont(io.BytesIO(data), recalcTimestamp=False)
    options = subset.Options()
    options.layout_features = ['*']
    options.name_IDs = ['*']
    options.name_legacy = True
    options.name_languages = ['*']
    sub = subset.Subsetter(options=options)
    sub.populate(text=text)
    sub.subset(source)
    names = {1: family, 2: 'Regular', 3: family + ' Regular ' + version, 4: family + ' Regular', 6: family.replace(' ', '') + '-Regular', 16: family, 17: 'Regular'}
    for record in source['name'].names:
        if record.nameID in names:
            record.string = names[record.nameID].encode(record.getEncoding())
    source['head'].modified = source['head'].created
    output = DEST / (key + ('.ttf' if member else '.otf'))
    source.save(output, reorderTables=True)
    (DEST / (key + '-OFL.txt')).write_bytes(license_data)
    manifest['fonts'].append({'fixture': output.name, 'family': family, 'version': version, 'source': url, 'archive_member': member,
        'archive_sha256': hashlib.sha256(archive).hexdigest(), 'source_sha256': hashlib.sha256(data).hexdigest(),
        'fixture_sha256': hashlib.sha256(output.read_bytes()).hexdigest(), 'license': key + '-OFL.txt',
        'license_sha256': hashlib.sha256(license_data).hexdigest(), 'text': text, 'face_index': 0})
(DEST / 'manifest.json').write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
print('Generated', len(SOURCES), 'renamed font subsets with shaping closure')
