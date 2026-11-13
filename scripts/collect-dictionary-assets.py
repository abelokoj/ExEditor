"""Collect pinned dictionary data and notices; never builds ExEditor or a spell checker."""
from pathlib import Path, PurePosixPath
import hashlib, json, re, shutil, tarfile, tempfile, urllib.parse, urllib.request, zipfile
import zstandard

PROJECT = Path(__file__).resolve().parent.parent
OUTPUT = PROJECT / "assets/dictionaries"
CACHE = Path(tempfile.gettempdir()) / "exeditor-dictionary-assets"
WM_CODE = "370d1e326427cdf98c6a777e3d72bfb0c27b5d27"
WM_DATA = "2c05d4a419a983724ca2479c87f04e712bc5559d"
HUNSPELL = "c0865a85eb67dc377f2cf8e8fe2c4fa9e53f0dcb"
OUTPUT.mkdir(parents=True, exist_ok=True)
CACHE.mkdir(parents=True, exist_ok=True)

def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def fetch(url, name, expected=None, size=None, limit=200*1024*1024):
    target=CACHE/name
    if target.is_file() and (expected is None or digest(target)==expected) and (size is None or target.stat().st_size==size): return target
    partial=target.with_suffix(target.suffix+".part")
    request=urllib.request.Request(url,headers={"User-Agent":"ExEditor-dictionary-assets"})
    with urllib.request.urlopen(request, timeout=90) as response, partial.open("wb") as output:
        total=0
        while chunk:=response.read(1024*1024):
            total+=len(chunk)
            if total>limit:raise ValueError("Dictionary source exceeds its download limit")
            output.write(chunk)
    if size is not None and partial.stat().st_size!=size:raise ValueError("Wrong size: "+name)
    if expected and digest(partial)!=expected:raise ValueError("Wrong digest: "+name)
    partial.replace(target)
    return target

def raw(repo,commit,path):
    return fetch(f"https://raw.githubusercontent.com/{repo}/{commit}/{path}", hashlib.sha256((repo+commit+path).encode()).hexdigest()+"-"+PurePosixPath(path).name)

def license_file(spdx):
    return raw("spdx/license-list-data","v3.25.0","text/"+spdx+".txt")

catalog=[]

def package(identifier,label,backend,language,version,primary,files,license,source_url,provenance,early=False):
    notice=(f"{label}\nVersion: {version}\nLicense: {license}\nSource: {source_url}\n\n"
        "Dictionary payloads are copied unchanged. Upstream copyright and license notices are included.\n"
        "For compiled spellers, the corresponding source archive is included under source/.\n\n"+json.dumps(provenance,indent=2)+"\n").encode()
    notice_path=CACHE/(identifier+"-NOTICE.txt");notice_path.write_bytes(notice)
    files={**files,"NOTICE.txt":notice_path}
    name=identifier+"-"+version.replace("/","-")+".zip"
    path=OUTPUT/name
    temporary=CACHE/(name+".new")
    entries=[]
    with zipfile.ZipFile(temporary,"w",compression=zipfile.ZIP_DEFLATED,compresslevel=9) as archive:
        for relative,source in sorted(files.items()):
            parts=PurePosixPath(relative)
            assert not parts.is_absolute() and all(part not in ("..","") for part in parts.parts)
            info=zipfile.ZipInfo(relative,date_time=(1980,1,1,0,0,0));info.compress_type=zipfile.ZIP_DEFLATED;info.external_attr=0o100644<<16
            with source.open("rb") as input,archive.open(info,"w") as output:shutil.copyfileobj(input,output,1024*1024)
            entries.append({"path":relative,"bytes":source.stat().st_size,"sha256":digest(source)})
    if temporary.stat().st_size>=100*1024*1024:raise ValueError("Dictionary package exceeds GitHub's per-file limit")
    if path.exists() and digest(path)!=digest(temporary):raise ValueError("Refusing to overwrite a different existing dictionary package: "+str(path))
    if not path.exists():shutil.copyfile(temporary,path)
    item={"id":identifier,"name":label,"backend":backend,"language":language,"version":version,"primary":primary,
          "license":license,"source_url":source_url,"pre_release":early,
          "archive":{"file":name,"bytes":path.stat().st_size,"sha256":digest(path)},"files":entries}
    catalog.append(item)
    print(json.dumps({"pack":identifier,"archive_bytes":path.stat().st_size,"installed_bytes":sum(entry['bytes'] for entry in entries)}),flush=True)

hunspell=[
 ("en-us","English (US)","en-US","en/en_US",["en/README_en_US.txt","en/README.txt","en/license.txt"],"SCOWL / BSD-style notices",[]),
 ("en-gb","English (UK)","en-GB","en/en_GB",["en/README_en_GB.txt","en/README.txt","en/license.txt"],"LGPL-3.0-or-later",["GPL-3.0-only","LGPL-3.0-only"]),
 ("fr-fr","French","fr-FR","fr_FR/dictionaries/fr",["fr_FR/dictionaries/README_dict_fr.txt"],"MPL-2.0",["MPL-2.0"]),
 ("de-de","German","de-DE","de/de_DE_frami",["de/README_de_DE_frami.txt","de/README_extension_owner.txt","de/COPYING_GPLv2","de/COPYING_GPLv3"],"GPL-2.0-only OR GPL-3.0-only",["GPL-2.0-only","GPL-3.0-only"]),
 ("es-es","Spanish","es-ES","es/es_ES",["es/README_hunspell_es.txt","es/LICENSE.md"],"GPL-3.0-or-later OR LGPL-3.0-or-later OR MPL-1.1-or-later",["GPL-3.0-only","LGPL-3.0-only","MPL-1.1"]),
 ("pt-br","Portuguese (Brazil)","pt-BR","pt_BR/pt_BR",["pt_BR/README_en.txt","pt_BR/README_pt_BR.txt"],"LGPL-3.0 / MPL (see included notices)",["GPL-3.0-only","LGPL-3.0-only","MPL-1.1"]),
 ("pt-pt","Portuguese (Portugal)","pt-PT","pt_PT/pt_PT",["pt_PT/README_pt_PT.txt","pt_PT/LICENSES.txt"],"GPL-2.0-or-later / BSD notices",["GPL-2.0-only"]),
]
for identifier,label,lang,stem,notices,license,licenses in hunspell:
    files={PurePosixPath(stem+extension).name:raw("LibreOffice/dictionaries",HUNSPELL,stem+extension) for extension in (".aff",".dic")}
    files.update({"notices/"+PurePosixPath(path).name:raw("LibreOffice/dictionaries",HUNSPELL,path) for path in notices})
    files.update({"licenses/"+name+".txt":license_file(name) for name in licenses})
    package("hunspell-"+identifier,label,"Hunspell",lang,"libreoffice-"+HUNSPELL[:12],PurePosixPath(stem+".dic").name,files,license,
        "https://github.com/LibreOffice/dictionaries/tree/"+HUNSPELL+"/"+stem.rsplit("/",1)[0],{"repository":"LibreOffice/dictionaries","commit":HUNSPELL,"files":[stem+".aff",stem+".dic"]})

# WM Keyboard's reproducible Finnish VFST dictionary, with its complete corresponding source.
voikko=raw("wasi-master/wmkeyboard-data",WM_DATA,"spell/voikko/voikko-fi-2.6-mor-standard.zip")
assert voikko.stat().st_size==1645402 and digest(voikko)=="9a7b8ed3f401ac46e81714775743219175c54164a4eaa3b0286c68db4825b439"
files={}
with zipfile.ZipFile(voikko) as archive:
    for member in archive.infolist():
        if member.is_dir():continue
        assert member.filename.startswith("5/mor-standard/") and ".." not in PurePosixPath(member.filename).parts
        path=CACHE/("voikko-"+PurePosixPath(member.filename).name);path.write_bytes(archive.read(member));files[member.filename]=path
for name in ["voikko-fi-2.6.tar.gz","voikko-fi-2.6.tar.gz.asc"]:
    files["source/"+name]=raw("wasi-master/wmkeyboard-data",WM_DATA,"spell/voikko/"+name)
files["source/WMKeyboard-build-dict.sh"]=raw("wasi-master/wmkeyboard",WM_CODE,"native/voikko-jni/build-dict.sh")
files["licenses/COPYING"]=raw("wasi-master/wmkeyboard-data",WM_DATA,"spell/voikko/COPYING")
# The build script stays under WM Keyboard's own repository license.
files["source/WMKeyboard-LICENSE"]=raw("wasi-master/wmkeyboard",WM_CODE,"LICENSE")
files["notices/WMKeyboard-README.md"]=raw("wasi-master/wmkeyboard-data",WM_DATA,"spell/voikko/README.md")
package("voikko-fi","Finnish","Voikko","fi","2.6",".",files,"GPL-2.0-or-later",
    "https://github.com/wasi-master/wmkeyboard-data/tree/"+WM_DATA+"/spell/voikko",{"repository":"wasi-master/wmkeyboard-data","commit":WM_DATA,"original_zip_sha256":digest(voikko),"builder_repository_commit":WM_CODE})

# Match only the literal, versioned entries; never execute the upstream Kotlin source.
kotlin=raw("wasi-master/wmkeyboard",WM_CODE,"core/prediction/src/main/java/com/wasimaster/wmkeyboard/core/dictionaries/DivvunSpellerCatalog.kt").read_text(encoding="utf-8")
pattern=r'entry\(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*(\d+),\s*"([0-9a-f]{64})",\s*([\d_]+),\s*"([^"]+)",\s*([\d_]+),\s*"([^"]+)"'
entries=re.findall(pattern,kotlin);assert len(entries)==10
names={"se":"North Sami","sma":"South Sami","smj":"Lule Sami","smn":"Inari Sami","sms":"Skolt Sami","kl":"Greenlandic","fo":"Faroese","olo":"Livvi Karelian","fkv":"Kven","vep":"Veps"}
for lang,code,version,build,sha,packed,member,unpacked,license in entries:
    url=f"https://github.com/giellalt/lang-{code}/releases/download/speller-{code}/v{version}/speller-{code}_{version}%2Bbuild.{build}_noarch-mobile.pkt.tar.zst"
    blob=fetch(url,f"divvun-{code}-{version}.pkt.tar.zst",sha,int(packed.replace("_","")))
    fst=CACHE/(f"divvun-{code}-{version}-"+member)
    if not fst.is_file() or fst.stat().st_size!=int(unpacked.replace("_","")):
        found=False
        with blob.open("rb") as input,zstandard.ZstdDecompressor().stream_reader(input) as decompressed,tarfile.open(fileobj=decompressed,mode="r|") as archive:
            for item in archive:
                if item.isfile() and PurePosixPath(item.name).name==member:
                    assert item.size==int(unpacked.replace("_",""))
                    with archive.extractfile(item) as input,fst.open("wb") as output:shutil.copyfileobj(input,output,1024*1024)
                    found=True;break
        assert found,member
    ref=urllib.parse.quote(f"speller-{code}/v{version}",safe="")
    head_path=fetch(f"https://api.github.com/repos/giellalt/lang-{code}/commits/{ref}",f"giella-{code}-{version}-commit.json",limit=2*1024*1024)
    source_commit=json.loads(head_path.read_text(encoding="utf-8"))["sha"]
    sources=fetch(f"https://codeload.github.com/giellalt/lang-{code}/tar.gz/{source_commit}",f"lang-{code}-{source_commit}.tar.gz")
    files={member:fst,"source/"+sources.name:sources}
    for spdx in (["GPL-3.0-only","LGPL-3.0-only"] if license.startswith("LGPL") else ["GPL-3.0-only"]):files["licenses/"+spdx+".txt"]=license_file(spdx)
    package("divvun-"+lang,names[lang],"Divvunspell",lang,version+"-build."+build,member,files,license,
        f"https://github.com/giellalt/lang-{code}/tree/{source_commit}",{"catalog_repository":"wasi-master/wmkeyboard","catalog_commit":WM_CODE,"url":url,"package_sha256":sha,"source_commit":source_commit},version.startswith("0."))

manifest={"version":1,"packs":catalog}
manifest_path=PROJECT/"assets/accessories/dictionaries.json"
manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
(OUTPUT/"README.md").write_text("# Optional spelling dictionary data\n\nThese archives are served by ExEditor's Settings/Language Tools downloads and are never embedded in the application or installer. Each contains unmodified dictionary files, notices and license texts. Compiled Voikko/Divvun packs also include corresponding source archives. No LibreOffice application or Android library is included.\n\nThe common Hunspell pairs come from LibreOffice/dictionaries at "+HUNSPELL+". Finnish Voikko and the catalog of ten released GiellaLT spellers follow WM Keyboard at "+WM_CODE+" and its data repository at "+WM_DATA+". Three versioned GiellaLT pre-releases are marked as preview in the catalog.\n\nRuntime downloads use the exact ExEditor GitHub build commit and per-file/ archive SHA-256 and size checks. Until these local files are committed and pushed, those ExEditor-hosted download links are not published.\n\nRun scripts/collect-dictionary-assets.py only to collect/repackage existing data; it does not compile a dictionary, engine or app. It requires Python and zstandard. The compiled-in catalog is assets/accessories/dictionaries.json. Its hashes cover every installed file, including source and license material.\n",encoding="utf-8")
print(json.dumps({"packs":len(catalog),"total_archive_bytes":sum(pack["archive"]["bytes"] for pack in catalog)}))
