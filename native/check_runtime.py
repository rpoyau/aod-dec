#!/usr/bin/env python3
"""Native ABI/reference differential corpus; seed is fixed and raw outcomes retained."""
import argparse, ctypes as C, hashlib, json, random, sys
from fractions import Fraction
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from runtime import dec_core as ref
P=C.c_void_p

def main():
 parser=argparse.ArgumentParser();parser.add_argument('library');parser.add_argument('--output',required=True);args=parser.parse_args()
 lib=C.CDLL(str(Path(args.library).resolve()))
 declarations={
 'dec_context_new':(P,[C.c_int64,C.c_int32]),'dec_context_free':(None,[P]),
 'dec_literal':(P,[P,C.c_char_p,C.c_char_p,C.c_uint64,C.c_int32]),
 'dec_builtin':(P,[P,C.c_char_p,C.POINTER(P),C.c_int32,C.c_int32]),
 'dec_binary':(P,[P,C.c_char_p,P,P,C.c_int32]),'dec_render':(P,[P,P]),
 'dec_render_size':(C.c_uint64,[P]),'dec_ok':(C.c_int32,[P]),'dec_error':(C.c_char_p,[P])}
 for name,(r,a) in declarations.items():getattr(lib,name).restype=r;getattr(lib,name).argtypes=a
 records=[]
 def run(name,inputs,expected,op=False):
  c=lib.dec_context_new(100000,128)
  def literal(v):
   if type(v) is str:t,b='str',v.encode()
   elif type(v) is bool:t,b='bool',str(v).lower().encode()
   else:t,b='int',str(v).encode()
   return lib.dec_literal(c,t.encode(),b,len(b),1)
  def call(n,values):
   arr=(P*len(values))(*values);return lib.dec_builtin(c,n.encode(),arr,len(values),1)
  def value(v):
   if type(v) is Fraction:return call('ratio',[literal(v.numerator),literal(v.denominator)])
   if type(v) in (ref.T1,ref.T32,ref.Q4):return call(ref.type_name(v),[literal(v.value)])
   if type(v) is ref.Trace:
    if v.parents:
     proposal=call('fold',[value(v.parents[0]),value(v.parents[1]),literal(v.scope),literal(v.lineage)])
     out=call('outer',[proposal,value(v.start)])
    else:out=call('trace',[literal(v.scope),literal(v.lineage),value(v.start)])
    ends={c.end for c in v.closures}
    for e in v.events:
     out=call('step',[out,value(ref.Q4(e.target)),value(ref.T1(e.orientation))]) if e.kind=='step' else call(e.kind,[out])
     if e.ordinal in ends:out=call('close',[out])
    return out
   if type(v) is ref.Fusion:return call('fold',[value(v.left),value(v.right),literal(v.scope),literal(v.lineage)])
   if type(v) is ref.Coupling:
    out=call('coupling',[value(v.left),value(v.right),literal(v.window)])
    for a,b in v.contacts:out=call('contact',[out,literal(a),literal(b)])
    return out
   return literal(v)
  handles=[value(v) for v in inputs]
  actual=lib.dec_binary(c,name.encode(),*handles,1) if op else call(name,handles)
  got=C.string_at(lib.dec_render(c,actual),lib.dec_render_size(c)).decode() if lib.dec_ok(c) else 'ERROR:'+lib.dec_error(c).decode()
  record={'operation':name,'inputs':[ref.render(v) for v in inputs],'expected':expected,'actual':got,'pass':got==expected}
  records.append(record);lib.dec_context_free(c)
 def case(name,inputs,op=False):
  try:want=ref.render(ref.apply_binary(name,*inputs) if op else ref.call_builtin(name,inputs))
  except ref.DecError as e:want='ERROR:line 1: '+str(e)
  run(name,inputs,want,op)
 rng=random.Random(13012026)
 for _ in range(128):
  a,b=rng.randrange(-2**200,2**200),rng.randrange(-2**100,2**100) or 1
  for op in ('+','-','*','/','//','%','==','<'):case(op,[a,b],True)
  qa,qb=Fraction(a,b),Fraction(b,a or 1)
  for op in ('+','-','*','/','>='):case(op,[qa,qb],True)
 for lane in range(32):
  for sign in (-1,0,1):
   t=ref.T32(sign*3**lane);case('pack',[t]);case('unpack',[t.bits()])
  case('unpack',[3<<(2*lane)])
 for t in (-ref.T32.LIMIT,ref.T32.LIMIT,0,1,-1):case('pack',[ref.T32(t)]);case('t32',[t])
 for t in (-(ref.T32.LIMIT+1),ref.T32.LIMIT+1):case('t32',[t])
 for q in range(-1,17):case('q4',[q])
 for t in (-2,-1,0,1,2):case('t1',[t])
 for a,b in ((-7,3),(7,-3),(-7,-3),(0,1),(1,0)):
  for op in ('//','%','/'):case(op,[a,b],True)
 for role in (-1,0,1):case('probability',[1,0,2,ref.T1(role)])
 for ws in ((0,0,0),(-1,1,3)):case('probability',[*ws,ref.T1(1)])
 for a,b in ((1,0),(2,-2**100),(0,-1)):case('ratio',[a,b])
 for v in (Fraction(1,2),Fraction(2**200),ref.T32(-12),ref.Q4(15)):case('int',[v])
 def closed(lineage,scope='B',count=1,leading=0):
  t=ref.new_trace(scope,lineage,ref.Q4(0))
  for _ in range(leading):t=ref.append_event(t,'dwell')
  for _ in range(count):
   t=ref.append_event(t,'step',ref.Q4(1),1);t=ref.append_event(t,'hinge');t=ref.append_event(t,'step',ref.Q4(0),-1);t=ref.close_trace(t)
  return t
 for label in ('plain','é😀','line\nquote"\\','nul\0DEL\x7f'):
  a,b=closed(label),closed(label+'-b');case('close',[a]);case('fold',[a,b,'outer',label]);case('trace',[label,label,ref.Q4(0)])
 a,b=closed('a'),closed('b');f=ref.fold(a,b,'outer','ab');out=ref.outer(f,ref.Q4(0))
 case('fuse',[f,out]);case('fuse',[f,closed('ab','outer')]);case('fold',[a,a,'outer','same']);case('fold',[a,b,'B','same'])
 out=ref.append_event(out,'step',ref.Q4(1),1);out=ref.append_event(out,'hinge');out=ref.append_event(out,'step',ref.Q4(0),-1);out=ref.close_trace(out);case('fuse',[f,out])
 case('fold',[out,b,'nested','nested-lineage'])
 a=ref.append_event(closed('a',count=2),'dwell');b=closed('b',count=2,leading=1);c=ref.coupling(a,b,'window')
 case('contact',[c,3,4]);c=ref.contact(c,3,4);case('contact',[c,3,4]);case('contact',[c,5,7]);case('cadence',[c]);case('contact',[c,6,7]);c=ref.contact(c,6,7)
 for n in ('cadence','lag','drift'):case(n,[c])
 for n in ('phases','beats','rate'):case(n,[a])
 result={'seed':13012026,'library_sha256':hashlib.sha256(Path(args.library).read_bytes()).hexdigest(),'cases':len(records),'failures':sum(not r['pass'] for r in records),'records':records}
 Path(args.output).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='records'}))
 return bool(result['failures'])
if __name__=='__main__':sys.exit(main())
