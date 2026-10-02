#!/usr/bin/env python3
from __future__ import annotations
import argparse,hashlib,io,json,struct
from collections import Counter
from pathlib import Path

SCHEMA = "chaptera.cfb-physical-diff.v1"

try:
 from .cfb_physical import CFB, END, FAT, FREE, NO, SIG, h
except ImportError:
 from cfb_physical import CFB, END, FAT, FREE, NO, SIG, h

def logical(b):
 import olefile
 out={}
 with olefile.OleFileIO(io.BytesIO(b)) as o:
  for p in o.listdir(streams=True,storages=False):
   q=o.openstream(p).read();out['/'+'/'.join(p)]={'len':len(q),'sha256':h(q)}
 return dict(sorted(out.items()))
def broad(x):
 if x.startswith('header.'):return 'header'
 if x.startswith('directory[') or x=='directory_sector':return 'directory'
 if x in ('fat_sector','difat_sector','minifat_sector'):return x.split('_')[0]
 if 'stream_payload:' in x:return 'stream_payload'
 if 'stream_slack:' in x:return 'stream_slack'
 if x=='root_ministream_container':return x
 if x=='unallocated_sector':return 'unallocated'
 if x=='allocated_other_sector':return 'allocated_other'
 return 'unclassified'
def ranges(offs,labs):
 if not offs:return []
 out=[];a=p=offs[0];x=labs[0]
 for o,y in zip(offs[1:],labs[1:]):
  if o==p+1 and y==x:p=o;continue
  out.append({'offset':a,'length':p-a+1,'category':x});a=p=o;x=y
 out.append({'offset':a,'length':p-a+1,'category':x});return out
def tablediff(a,b,n):return [{'index':i,'left':a[i] if i<len(a) else None,'right':b[i] if i<len(b) else None} for i in range(n) if (a[i] if i<len(a) else None)!=(b[i] if i<len(b) else None)]
def chainmap(m):return {(e['i'],e['name']):{'storage':'minifat' if e['size']<m.cut else 'fat','chain':m.schain(e),'size':e['size']} for e in m.dirs if e['type']==2 and e['size']}
def compare(a,b,n=''):
 if len(a)!=len(b) or h(a)==h(b):raise ValueError('pair identity/length invariant')
 if logical(a)!=logical(b):raise ValueError('logical streams differ')
 l=CFB(a);r=CFB(b);off=[i for i,(x,y) in enumerate(zip(a,b)) if x!=y];exact=[];wide=[];unc=0
 for i in off:
  x,y=l.lab[i],r.lab[i];e=x if x==y else x+'->'+y;u,v=broad(x),broad(y);w=u if u==v else u+'->'+v;exact.append(e);wide.append(w);unc+=('unclassified' in w)
 ld={e['i']:{k:v for k,v in e.items() if k!='raw'} for e in l.dirs if e['type']};rd={e['i']:{k:v for k,v in e.items() if k!='raw'} for e in r.dirs if e['type']};dd=[]
 for i in sorted(set(ld)|set(rd)):
  x,y=ld.get(i,{}),rd.get(i,{});c={k:{'left':x.get(k),'right':y.get(k)} for k in sorted(set(x)|set(y)) if x.get(k)!=y.get(k)}
  if c:dd.append({'index':i,'changed_fields':c})
 lc,rc=chainmap(l),chainmap(r);cd=[]
 for k in sorted(set(lc)|set(rc),key=lambda q:(q[0],q[1].casefold(),q[1])):
  if lc.get(k)!=rc.get(k):cd.append({'directory_index':k[0],'name':k[1],'left':lc.get(k),'right':rc.get(k)})
 return {'schema':SCHEMA,'name':n,'left_sha256':h(a),'right_sha256':h(b),'byte_len':len(a),'logical_stream_count':len(logical(a)),'logical_streams_identical':True,'different_byte_count':len(off),'classified_byte_count':len(off)-unc,'unclassified_byte_count':unc,'broad_category_counts':dict(sorted(Counter(wide).items())),'exact_category_counts':dict(sorted(Counter(exact).items())),'diff_ranges':ranges(off,exact),'left_physical':l.summary(),'right_physical':r.summary(),'directory_metadata_diffs':dd,'fat_table_diff_entries':tablediff(l.fat,r.fat,max(l.nsec,r.nsec)),'minifat_table_diff_entries':tablediff(l.minifat,r.minifat,max(len(l.minifat),len(r.minifat))),'stream_chain_diffs':cd,'fat_sector_ids_equal':l.fatsecs==r.fatsecs,'difat_sector_ids_equal':l.difsecs==r.difsecs,'directory_sector_ids_equal':l.dirsecs==r.dirsecs,'minifat_sector_ids_equal':l.minisecs==r.minisecs,'root_chain_equal':l.rootchain==r.rootchain}
def minimal(state=0):
 s=512;b=bytearray(s*11);b[:8]=SIG
 for o,v,fmt in [(24,0x3e,'H'),(26,3,'H'),(28,0xfffe,'H'),(30,9,'H'),(32,6,'H'),(44,1,'I'),(48,0,'I'),(56,4096,'I'),(60,END,'I'),(68,END,'I')]:struct.pack_into('<'+fmt,b,o,v)
 for i in range(109):struct.pack_into('<I',b,76+4*i,FREE)
 struct.pack_into('<I',b,76,9)
 def de(o,n,t,ch,st,z,state=0):
  e=(n+'\0').encode('utf-16le');b[o:o+len(e)]=e;struct.pack_into('<H',b,o+64,len(e));b[o+66]=t;b[o+67]=1
  for p,v in [(68,NO),(72,NO),(76,ch),(96,state),(116,st)]:struct.pack_into('<I',b,o+p,v)
  struct.pack_into('<Q',b,o+120,z)
 de(512,'Root Entry',5,1,END,0);de(640,'Data',2,NO,1,4096,state)
 q=bytes(i%251 for i in range(4096))
 for j,sid in enumerate(range(1,9)):b[(sid+1)*512:(sid+2)*512]=q[j*512:(j+1)*512]
 v=[FREE]*128;v[0]=END
 for sid in range(1,8):v[sid]=sid+1
 v[8]=END;v[9]=FAT;struct.pack_into('<'+'I'*128,b,5120,*v);return bytes(b)
def selftest():
 a,b=minimal(0),minimal(1);x,y=CFB(a),CFB(b);d=[i for i,(p,q) in enumerate(zip(a,b)) if p!=q];assert d==[736] and x.lab[d[0]]==y.lab[d[0]]=='directory[1].state'
 r=compare(a,b,'self-test');assert r['schema']==SCHEMA and r['different_byte_count']==1 and r['unclassified_byte_count']==0
 fake=CFB.__new__(CFB);fake.nsec=10;fake.fat=[END]*10;mini=[FREE]*32;mini[20]=21;mini[21]=END
 assert fake.chain(20,mini,'mini-regression')==[20,21]
 legacy=bytearray(minimal(0));struct.pack_into('<I',legacy,640+124,0xffffffff);z=CFB(bytes(legacy))
 assert z.dirs[1]['size']==4096 and z.dirs[1]['size_high']==0xffffffff
 print('cfb physical diff self-test ok');return 0
def main():
 p=argparse.ArgumentParser();s=p.add_subparsers(dest='cmd',required=True);c=s.add_parser('compare');c.add_argument('--left',type=Path,required=True);c.add_argument('--right',type=Path,required=True);c.add_argument('--name',default='');c.add_argument('--out',type=Path,required=True);s.add_parser('self-test');a=p.parse_args()
 if a.cmd=='self-test':return selftest()
 r=compare(a.left.read_bytes(),a.right.read_bytes(),a.name);a.out.parent.mkdir(parents=True,exist_ok=True);a.out.write_text(json.dumps(r,indent=2,ensure_ascii=False));print(json.dumps({k:r[k] for k in ['name','different_byte_count','unclassified_byte_count','broad_category_counts']},indent=2));return 0
if __name__=='__main__':raise SystemExit(main())
