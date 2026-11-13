#include "runtime.hpp"
#include <gmpxx.h>
#include <openssl/sha.h>
#include <algorithm>
#include <memory>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>
#include <cstdio>
#include <limits>

// Runtime data never escapes this translation unit. Arena handles are immutable.
namespace {
void req(bool b,const std::string& s) { if(!b) throw std::runtime_error(s); }
struct Event { size_t ordinal; std::string kind; int source,target,orientation; };
struct Closure { size_t begin,end; std::string digest; };
struct Trace { std::string scope,lineage; int start=0; std::vector<Event> events; std::vector<Closure> closures; std::vector<std::shared_ptr<const Trace>> parents; std::string proposal; };
struct Fusion { std::shared_ptr<const Trace> left,right; std::string scope,lineage,digest; };
struct Coupling { std::shared_ptr<const Trace> left,right; std::string window; std::vector<std::pair<mpz_class,mpz_class>> contacts; };
struct Value { std::string type; mpq_class n; std::string s; std::shared_ptr<const Trace> t; std::shared_ptr<const Fusion> f; std::shared_ptr<const Coupling> c; };
struct Context { int64_t fuel; int depth=0,depth_limit; std::string error,output,scratch; bool failed=false; int line=0; std::vector<std::unique_ptr<const Value>> arena; std::set<const Value*> owned; uint64_t allocation_limit=std::numeric_limits<uint64_t>::max(); };
std::string hex4(unsigned n) { char b[7]; std::snprintf(b,sizeof b,"\\u%04x",n); return b; }
// Python json.dumps ensure_ascii=True spelling, including surrogate pairs.
std::string quote(const std::string& s) {
 std::string out="\"";
 for(size_t i=0;i<s.size();) {
  unsigned c=(unsigned char)s[i++];
  if(c>=0x80) {
   unsigned more=0,cp=0;
   if((c&0xe0)==0xc0){more=1;cp=c&31;} else if((c&0xf0)==0xe0){more=2;cp=c&15;} else if((c&0xf8)==0xf0){more=3;cp=c&7;} else throw std::runtime_error("invalid UTF-8 string");
   req(i+more<=s.size(),"invalid UTF-8 string");
   for(unsigned j=0;j<more;j++){unsigned d=(unsigned char)s[i++];req((d&0xc0)==0x80,"invalid UTF-8 string");cp=(cp<<6)|(d&63);}
   req(cp<=0x10ffff,"invalid UTF-8 string");
   if(cp<=0xffff)out+=hex4(cp);else{cp-=0x10000;out+=hex4(0xd800+(cp>>10));out+=hex4(0xdc00+(cp&1023));}
  } else if(c=='"')out+="\\\"";else if(c=='\\')out+="\\\\";
  else if(c=='\b')out+="\\b";else if(c=='\f')out+="\\f";else if(c=='\n')out+="\\n";else if(c=='\r')out+="\\r";else if(c=='\t')out+="\\t";
  else if(c<32||c==127)out+=hex4(c);else out+=(char)c;
 }
 return out+'"';
}
std::string hash(const std::string& s){unsigned char d[SHA256_DIGEST_LENGTH];SHA256((const unsigned char*)s.data(),s.size(),d);std::string o;for(auto c:d){char h[3];std::snprintf(h,3,"%02x",c);o+=h;}return o;}
template<class T,class F> std::string array(const std::vector<T>& a,F f){std::string s="[";for(size_t i=0;i<a.size();i++){if(i)s+=',';s+=f(a[i]);}return s+"]";}
std::string event_json(const Event&e){return "{\"kind\":"+quote(e.kind)+",\"ordinal\":"+std::to_string(e.ordinal)+",\"orientation\":"+std::to_string(e.orientation)+",\"source\":"+std::to_string(e.source)+",\"target\":"+std::to_string(e.target)+"}";}
std::string trace_json(const Trace&);
std::string trace_json(const Trace&t){return "{\"closures\":"+array(t.closures,[](auto&c){return "{\"begin\":"+std::to_string(c.begin)+",\"digest\":"+quote(c.digest)+",\"end\":"+std::to_string(c.end)+"}";})+",\"events\":"+array(t.events,event_json)+",\"lineage\":"+quote(t.lineage)+",\"parents\":"+array(t.parents,[](auto&p){return trace_json(*p);})+",\"proposal\":"+quote(t.proposal)+",\"scope\":"+quote(t.scope)+",\"start\":{\"value\":"+std::to_string(t.start)+"}}";}
std::string prefix(const Trace&t,size_t end){std::vector<Event> e(t.events.begin(),t.events.begin()+end);return "{\"events\":"+array(e,event_json)+",\"lineage\":"+quote(t.lineage)+",\"parents\":"+array(t.parents,[](auto&p){return quote(p->closures.back().digest);})+",\"proposal\":"+quote(t.proposal)+",\"scope\":"+quote(t.scope)+",\"start\":"+std::to_string(t.start)+"}";}
std::string fusion_digest(const Trace&a,const Trace&b,const std::string&s,const std::string&l){return hash("{\"lineage\":"+quote(l)+",\"parents\":["+quote(a.closures.back().digest)+","+quote(b.closures.back().digest)+"],\"scope\":"+quote(s)+"}");}
void label(const std::string&s){
 // Python str.strip whitespace set, expressed as Unicode scalar values.
 bool nonwhite=false;
 for(size_t i=0;i<s.size();){unsigned c=(unsigned char)s[i++]; if(c>=128){unsigned k=(c&0xe0)==0xc0?1:(c&0xf0)==0xe0?2:3;unsigned cp=c&((1u<<(6-k))-1);for(unsigned j=0;j<k && i<s.size();j++)cp=(cp<<6)|((unsigned char)s[i++]&63);c=cp;}bool ws=(c>=9&&c<=13)||(c>=28&&c<=32)||c==0x85||c==0xa0||c==0x1680||(c>=0x2000&&c<=0x200a)||c==0x2028||c==0x2029||c==0x202f||c==0x205f||c==0x3000;if(!ws)nonwhite=true;}
 req(nonwhite,"scope, lineage and window names must be nonempty");
}
int segment(const Trace&t,size_t begin,size_t end,bool complete){
 req(begin<=end&&end<=t.events.size(),"invalid trace segment");int p=t.start,orientation=0,moves=0;bool hinged=false;
 for(size_t i=begin;i<end;i++){auto&e=t.events[i];req(e.ordinal==i+1,"invalid or repeated event identity");req(e.source==p,"discontinuous retained trace");req(e.target>=0&&e.target<16,"q4 outside 0..15");
  if(e.kind=="step"){req(e.orientation==-1||e.orientation==1,"step requires nonzero orientation");req(__builtin_popcount((unsigned)(p^e.target))==1,"atomic move must be Hamming-1");if(!orientation){req(!hinged,"hinge cannot precede outgoing branch");orientation=e.orientation;}req(e.orientation==(hinged?-orientation:orientation),"counterbranch orientation requires structural hinge");moves++;p=e.target;}
  else if(e.kind=="hinge"){req(orientation&&!hinged,"one structural hinge must follow outgoing branch");req(e.target==p&&!e.orientation,"invalid structural hinge");hinged=true;}
  else if(e.kind=="dwell")req(e.target==p&&!e.orientation,"invalid dwell");else throw std::runtime_error("unknown retained event kind");
 }
 if(complete){req(moves>=2&&hinged,"closure needs outgoing branch, hinge and counterbranch");req(p==t.start,"open trace does not return to retained start");req(t.events[end-1].kind=="step","closure must identify a return event, not dwell");req(t.events[end-1].orientation==-orientation,"return event is not counterbranch");}return p;
}
int validate(const Trace&t,int depth=0){
 req(depth<64,"invalid or excessively nested trace");label(t.scope);label(t.lineage);req(t.start>=0&&t.start<16,"q4 outside 0..15");
 if(!t.parents.empty()){req(t.parents.size()==2,"fusion requires two retained parents in this subset");for(auto&p:t.parents){req(bool(p),"invalid or excessively nested trace");validate(*p,depth+1);req(!p->closures.empty()&&p->closures.back().end==p->events.size(),"fusion parent is not closed");}auto&a=*t.parents[0];auto&b=*t.parents[1];req(a.scope!=b.scope||a.lineage!=b.lineage,"fusion parents must have distinct lineages");req(t.scope!=a.scope&&t.scope!=b.scope,"outer scope must differ from inner scopes");req(t.proposal==fusion_digest(a,b,t.scope,t.lineage),"outer trace does not match retained fusion proposal");}else req(t.proposal.empty(),"orphan fusion proposal");
 size_t begin=0;for(auto&c:t.closures){req(c.begin==begin&&begin<c.end&&c.end<=t.events.size(),"invalid/replayed closure range");segment(t,begin,c.end,true);req(c.digest==hash(prefix(t,c.end)),"closure certificate does not match retained history");begin=c.end;}return segment(t,begin,t.events.size(),false);
}
Fusion fold(std::shared_ptr<const Trace>a,std::shared_ptr<const Trace>b,const std::string&s,const std::string&l){for(auto&t:{a,b}){validate(*t);req(!t->closures.empty()&&t->closures.back().end==t->events.size(),"fold requires closed retained inner inputs");}req(a->scope!=b->scope||a->lineage!=b->lineage,"fold requires distinct inner lineages");label(s);label(l);req(s!=a->scope&&s!=b->scope,"outer scope must differ from inner scopes");return {a,b,s,l,fusion_digest(*a,*b,s,l)};}
void validate(const Fusion&f){auto e=fold(f.left,f.right,f.scope,f.lineage);req(e.digest==f.digest,"modified fusion proposal");}
Coupling coupling(std::shared_ptr<const Trace>a,std::shared_ptr<const Trace>b,const std::string&w){validate(*a);validate(*b);label(w);req(a->scope==b->scope&&a->lineage!=b->lineage,"coupling needs distinct lineages in common scope");req(a->events.size()==b->events.size()&&!a->events.empty(),"coupling needs equal positive committed event spans");return {a,b,w,{}};}
void validate(const Coupling&c){coupling(c.left,c.right,c.window);std::set<mpz_class>a,b;for(auto&x:c.left->closures)a.insert(mpz_class(x.end));for(auto&x:c.right->closures)b.insert(mpz_class(x.end));mpz_class pa=0,pb=0;for(auto&p:c.contacts){req(a.count(p.first)&&b.count(p.second),"contact must bind certified return events");req(p.first>pa&&p.second>pb,"contacts must be distinct, ordered and unreplayed");pa=p.first;pb=p.second;}}
void comparison(const Coupling&c){validate(c);req(c.contacts.size()>=2&&c.left->closures.size()>=2&&c.right->closures.size()>=2,"comparison needs recurrent lineages and repeated bound contacts");}
Value numeric(const std::string&t,const mpq_class&n){
 Value v;v.type=t;mpz_class numerator=n.get_num(),denominator=n.get_den();req(denominator!=0,"zero denominator");if(denominator<0){numerator=-numerator;denominator=-denominator;}v.n=mpq_class(numerator,denominator);v.n.canonicalize();
 if(t!="ratio")req(v.n.get_den()==1,"expected exact int");
 auto z=v.n.get_num();if(t=="t1")req(z>=-1&&z<=1,"t1 outside {-1,0,+1}");if(t=="t32"){static const mpz_class lim("926510094425920");req(z>=-lim&&z<=lim,"t32 overflow");}if(t=="q4")req(z>=0&&z<16,"q4 outside 0..15");return v;
}
Value truth(bool b){return numeric("bool",mpq_class(b?1:0));}
Value tv(const Trace&t){Value v;v.type="trace";v.t=std::make_shared<const Trace>(t);return v;}
std::string render(const Value&v){if(v.type=="str")return v.s;if(v.type=="bool")return v.n==0?"false":"true";if(v.type=="int")return v.n.get_num().get_str();if(v.type=="ratio")return v.n.get_num().get_str()+"/"+v.n.get_den().get_str();if(v.type=="t1"||v.type=="t32"||v.type=="q4")return v.type+"("+v.n.get_num().get_str()+")";if(v.type=="trace")return trace_json(*v.t);if(v.type=="fusion"){auto&f=*v.f;return "{\"digest\":"+quote(f.digest)+",\"left\":"+trace_json(*f.left)+",\"lineage\":"+quote(f.lineage)+",\"right\":"+trace_json(*f.right)+",\"scope\":"+quote(f.scope)+"}";}if(v.type=="coupling"){auto&c=*v.c;return "{\"contacts\":"+array(c.contacts,[](auto&p){return "["+p.first.get_str()+","+p.second.get_str()+"]";})+",\"left\":"+trace_json(*c.left)+",\"right\":"+trace_json(*c.right)+",\"window\":"+quote(c.window)+"}";}throw std::runtime_error("unsupported value type");}
Value unary(const std::string&op,const Value&v){if(op=="not"){req(v.type=="bool","invalid unary operation");return truth(v.n==0);}req(v.type=="int"||v.type=="t1"||v.type=="t32"||v.type=="ratio","unary arithmetic requires numeric type");req(op=="+"||op=="-","unsupported unary operator");return numeric(v.type,op=="+"?v.n:mpq_class(-v.n));}
Value binary(const std::string&op,const Value&a,const Value&b){req(a.type==b.type,"implicit numeric coercion is forbidden");if(op=="=="||op=="!="){bool eq=render(a)==render(b);return truth(op=="=="?eq:!eq);}req(a.type=="int"||a.type=="t1"||a.type=="t32"||a.type=="ratio","arithmetic requires numeric type");if(op=="<")return truth(a.n<b.n);if(op=="<=")return truth(a.n<=b.n);if(op==">")return truth(a.n>b.n);if(op==">=")return truth(a.n>=b.n);if(op=="/"){req(b.n!=0,"zero denominator");return numeric("ratio",a.n/b.n);}if(op=="+")return numeric(a.type,a.n+b.n);if(op=="-")return numeric(a.type,a.n-b.n);if(op=="*")return numeric(a.type,a.n*b.n);req(op=="//"||op=="%","unsupported binary operator");req(a.type=="int"||a.type=="t32","integer quotient/remainder required");req(b.n!=0,"zero divisor");mpz_class q,r;auto az=a.n.get_num(),bz=b.n.get_num();mpz_fdiv_qr(q.get_mpz_t(),r.get_mpz_t(),az.get_mpz_t(),bz.get_mpz_t());return numeric(a.type,mpq_class(op=="//"?q:r));}
void signatures(const std::string&name,const std::vector<const Value*>&a){
 std::string s;for(auto*v:a){if(!s.empty())s+=',';s+=v->type;}bool ok=false;
 if(name=="t1"||name=="t32"||name=="q4"||name=="unpack")ok=s=="int";
 else if(name=="int")ok=s=="int"||s=="t1"||s=="t32"||s=="q4"||s=="ratio";
 else if(name=="ratio")ok=s=="int"||s=="int,int";else if(name=="pack")ok=s=="t32";
 else if(name=="trace")ok=s=="str,str,q4";else if(name=="step")ok=s=="trace,q4,t1";
 else if(name=="hinge"||name=="dwell"||name=="close"||name=="phases"||name=="beats"||name=="rate")ok=s=="trace";
 else if(name=="fold")ok=s=="trace,trace,str,str";else if(name=="outer")ok=s=="fusion,q4";else if(name=="fuse")ok=s=="fusion,trace";
 else if(name=="coupling")ok=s=="trace,trace,str";else if(name=="contact")ok=s=="coupling,int,int";
 else if(name=="cadence"||name=="lag"||name=="drift")ok=s=="coupling";else if(name=="probability")ok=s=="int,int,int,t1";
 else if(name=="neg")ok=s=="int"||s=="t1"||s=="t32"||s=="ratio";else throw std::runtime_error("unknown builtin: "+name);
 req(ok,"unlisted signature for "+name+"("+s+")");
}
Value builtin(const std::string&name,const std::vector<const Value*>&a){signatures(name,a);auto&v=*a[0];
 if(name=="t1"||name=="t32"||name=="q4")return numeric(name,v.n);
 if(name=="int"){req(v.n.get_den()==1,"ratio-to-int would lose a remainder");return numeric("int",v.n);}
 if(name=="ratio"){mpq_class d=a.size()==2?a[1]->n:mpq_class(1);req(d!=0,"zero denominator");return numeric("ratio",v.n/d);}
 if(name=="neg")return unary("-",v);
 if(name=="pack"){mpz_class z=v.n.get_num(),bits=0;for(unsigned i=0;i<32;i++){unsigned long r=mpz_fdiv_ui(z.get_mpz_t(),3);int digit=r==2?-1:(int)r;z=(z-digit)/3;mpz_class code=digit==-1?2:digit;bits+=code<<(2*i);}req(z==0,"t32 packing overflow");return numeric("int",mpq_class(bits));}
 if(name=="unpack"){mpz_class bits=v.n.get_num(),limit=mpz_class(1)<<64;req(bits>=0&&bits<limit,"packed t32 outside u64");mpz_class z=0,power=1;for(unsigned i=0;i<32;i++){unsigned code=(bits.get_ui()>>(2*i))&3;req(code!=3,"11 is invalid trit encoding; it is not Null");z+=(code==2?-1:(int)code)*power;power*=3;}return numeric("t32",mpq_class(z));}
 if(name=="probability"){mpq_class sum=0;for(int i=0;i<3;i++){req(a[i]->n>=0,"weights must be exact and nonnegative");sum+=a[i]->n;}req(sum>0,"normalization requires positive total weight");return numeric("ratio",a[a[3]->n.get_num().get_si()+1]->n/sum);}
 if(name=="trace"){Trace t;t.scope=v.s;t.lineage=a[1]->s;t.start=a[2]->n.get_num().get_si();validate(t);return tv(t);}
 if(name=="step"||name=="hinge"||name=="dwell"){auto t=*v.t;int p=validate(t);t.events.push_back({t.events.size()+1,name,p,name=="step"?(int)a[1]->n.get_num().get_si():p,name=="step"?(int)a[2]->n.get_num().get_si():0});validate(t);return tv(t);}
 if(name=="close"){auto t=*v.t;validate(t);size_t begin=t.closures.empty()?0:t.closures.back().end;if(begin==t.events.size()&&!t.closures.empty())return v;segment(t,begin,t.events.size(),true);t.closures.push_back({begin,t.events.size(),hash(prefix(t,t.events.size()))});validate(t);return tv(t);}
 if(name=="phases"||name=="beats"||name=="rate"){auto&t=*v.t;validate(t);size_t n=t.closures.size();if(name=="beats")n=n?n-1:0;if(name=="rate"){req(!t.events.empty(),"rate requires a positive committed event span");return numeric("ratio",mpq_class(mpz_class(n),mpz_class(t.events.size())));}return numeric("int",mpq_class(mpz_class(n)));}
 if(name=="fold"){Value o;o.type="fusion";o.f=std::make_shared<const Fusion>(fold(v.t,a[1]->t,a[2]->s,a[3]->s));return o;}
 if(name=="outer"){auto&f=*v.f;validate(f);Trace t;t.scope=f.scope;t.lineage=f.lineage;t.start=a[1]->n.get_num().get_si();t.parents={f.left,f.right};t.proposal=f.digest;validate(t);return tv(t);}
 if(name=="fuse"){auto&f=*v.f;auto&t=*a[1]->t;validate(f);validate(t);req(t.scope==f.scope&&t.lineage==f.lineage&&t.parents.size()==2&&trace_json(*t.parents[0])==trace_json(*f.left)&&trace_json(*t.parents[1])==trace_json(*f.right)&&t.proposal==f.digest,"outer trace belongs to a different proposal");req(!t.closures.empty()&&t.closures.back().end==t.events.size(),"fusion requires independently witnessed outer closure");return *a[1];}
 if(name=="coupling"){Value o;o.type="coupling";o.c=std::make_shared<const Coupling>(coupling(v.t,a[1]->t,a[2]->s));return o;}
 if(name=="contact"){auto c=*v.c;validate(c);c.contacts.push_back({a[1]->n.get_num(),a[2]->n.get_num()});validate(c);Value o;o.type="coupling";o.c=std::make_shared<const Coupling>(c);return o;}
 auto&c=*v.c;comparison(c);if(name=="cadence")return numeric("ratio",mpq_class(mpz_class(c.left->closures.size()),mpz_class(c.right->closures.size())));mpz_class n=c.contacts.back().first-c.contacts.back().second;if(name=="drift")n-=c.contacts.front().first-c.contacts.front().second;return numeric("int",mpq_class(n));
}
const Value& value(Context*c,void*p){req(p&&c->owned.count((Value*)p),"invalid or foreign value handle");return *(Value*)p;}
void error(Context*c,const char*s,int line) noexcept {try{if(c&&!c->failed){c->failed=true;c->line=line;c->error=line>0?"line "+std::to_string(line)+": "+s:s;}}catch(...){/* allocation exhaustion cannot propagate over C */}}
template<class F> void* protect(void*p,int line,F f) noexcept {auto*c=(Context*)p;if(!c||c->failed)return nullptr;try{req(c->arena.size()<c->allocation_limit,"runtime allocation limit exceeded");auto v=std::make_unique<const Value>(f(c));auto*r=v.get();c->arena.push_back(std::move(v));c->owned.insert(r);return (void*)r;}catch(const std::bad_alloc&){error(c,"runtime allocation failed",line);}catch(const std::exception&e){error(c,e.what(),line);}catch(...){error(c,"native runtime failure",line);}return nullptr;}
}
extern "C" {
void* dec_context_new(int64_t fuel,int32_t depth){try{auto*c=new Context;c->fuel=fuel;c->depth_limit=depth;return c;}catch(...){return nullptr;}}
void dec_context_free(void*p){delete (Context*)p;}
int32_t dec_ok(void*p){return p&&!((Context*)p)->failed;}
const char* dec_error(void*p){if(!p)return "runtime allocation failed";auto*c=(Context*)p;return c->failed&&c->error.empty()?"runtime allocation failed":c->error.c_str();}
int32_t dec_error_line(void*p){return p?((Context*)p)->line:0;}
int32_t dec_tick(void*p){auto*c=(Context*)p;if(!dec_ok(p))return 0;if(c->fuel<=0){error(c,"execution step budget exhausted",0);return 0;}--c->fuel;return 1;}
int32_t dec_enter(void*p){auto*c=(Context*)p;if(!dec_ok(p))return 0;c->depth++;if(c->depth>c->depth_limit){error(c,"call depth limit exceeded",0);return 0;}return 1;}
void dec_leave(void*p){if(p&&((Context*)p)->depth>0)((Context*)p)->depth--;}
void* dec_literal(void*p,const char*t,const char*s,uint64_t n,int32_t line){return protect(p,line,[&](auto*){req(t&&s,"null literal");std::string type(t),text(s,n);if(type=="str"){Value v;v.type=type;v.s=text;return v;}if(type=="bool"){req(text=="true"||text=="false"||text=="0"||text=="1","invalid bool literal");return truth(text=="true"||text=="1");}req(type=="int","invalid literal type");mpz_class z;req(z.set_str(text,10)==0,"invalid integer literal");return numeric(type,mpq_class(z));});}
void* dec_unary(void*p,const char*op,void*v,int32_t line){return protect(p,line,[&](auto*c){req(op,"null operator");return unary(op,value(c,v));});}
void* dec_binary(void*p,const char*op,void*a,void*b,int32_t line){return protect(p,line,[&](auto*c){req(op,"null operator");return binary(op,value(c,a),value(c,b));});}
void* dec_builtin(void*p,const char*name,void**args,int32_t n,int32_t line){return protect(p,line,[&](auto*c){req(name&&n>=0&&(n==0||args),"invalid builtin call");std::vector<const Value*> a;for(int i=0;i<n;i++)a.push_back(&value(c,args[i]));return builtin(name,a);});}
int32_t dec_truth(void*p,void*v){if(!dec_ok(p))return 0;auto*c=(Context*)p;try{auto&a=value(c,v);req(a.type=="bool","condition requires bool");return a.n!=0;}catch(const std::exception&e){error(c,e.what(),0);}catch(...){error(c,"native runtime failure",0);}return 0;}
void dec_emit(void*p,void*v,int32_t line){if(!dec_ok(p))return;auto*c=(Context*)p;try{c->output+=render(value(c,v))+"\n";}catch(const std::exception&e){error(c,e.what(),line);}catch(...){error(c,"native runtime failure",line);}}
void dec_assert(void*p,void*v,int32_t line){if(!dec_ok(p))return;if(!dec_truth(p,v)&&dec_ok(p))error((Context*)p,"assertion failed",line);}

const char* dec_render(void*p,void*v){if(!dec_ok(p))return "";auto*c=(Context*)p;try{c->scratch=render(value(c,v));return c->scratch.c_str();}catch(const std::exception&e){error(c,e.what(),0);}catch(...){error(c,"native runtime failure",0);}return "";}
uint64_t dec_render_size(void*p){return p?((Context*)p)->scratch.size():0;}
const char* dec_output(void*p){return p?((Context*)p)->output.c_str():"";}
uint64_t dec_output_size(void*p){return p?((Context*)p)->output.size():0;}
void dec_set_allocation_limit(void*p,uint64_t n){if(p)((Context*)p)->allocation_limit=n;}
}
