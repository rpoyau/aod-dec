// White-box hostile mutations are confined to this test translation unit.
// Production ABI never exposes mutable records.
#include "runtime.cpp"
#include <iostream>
#include <functional>

void check(bool b,const char*why){if(!b)throw std::runtime_error(why);}
void rejects(std::function<void()> f,const char*part){try{f();}catch(const std::runtime_error&e){check(std::string(e.what()).find(part)!=std::string::npos,e.what());return;}throw std::runtime_error("accepted hostile mutation");}
Trace closed(std::string lineage="a",std::string scope="B") {Trace t;t.scope=scope;t.lineage=lineage;t.events={{1,"step",0,1,1},{2,"hinge",1,1,0},{3,"step",1,0,-1}};t.closures={{0,3,hash(prefix(t,3))}};validate(t);return t;}
int main(){try{
 auto t=closed();check(t.closures[0].digest=="9f954a87c82157ad4a2844d9dd242df35f3a50867fd6e391658c34068a461d7a","bound reference digest");

 check(t.events.size()==3&&t.events[0].orientation+t.events[1].orientation+t.events[2].orientation==0,"cancellation erased event");
 for(int k=0;k<7;k++){auto m=t;if(k==0)m.scope="wrong";if(k==1)m.lineage="wrong";if(k==2)m.start=2;if(k==3)m.events.back().ordinal=2;if(k==4)m.closures.push_back(m.closures[0]);if(k==5)m.closures[0].digest="forged";if(k==6)m.events.pop_back();rejects([&]{validate(m);},"");}
 auto open=t;open.closures.clear();open.events.pop_back();rejects([&]{segment(open,0,open.events.size(),true);},"closure needs");
 auto a=std::make_shared<const Trace>(t),b=std::make_shared<const Trace>(closed("b"));auto f=fold(a,b,"outer","ab");
 auto forged=f;forged.digest="fake";rejects([&]{validate(forged);},"modified fusion");
 auto outer=t;outer.scope="outer";outer.lineage="ab";outer.parents={a,b};outer.proposal=f.digest;outer.closures[0].digest=hash(prefix(outer,3));validate(outer);
 auto wrong=outer;wrong.parents={b,a};rejects([&]{validate(wrong);},"fusion proposal");
 wrong=outer;wrong.scope="B";rejects([&]{validate(wrong);},"outer scope");
 auto cc=coupling(a,b,"window");cc.contacts={{3,3}};validate(cc);cc.contacts.push_back({3,3});rejects([&]{validate(cc);},"unreplayed");cc.contacts={{2,3}};rejects([&]{validate(cc);},"certified return");
 // Parent history remains alive through many outer arena objects.
 for(int i=0;i<4;i++){auto aa=std::make_shared<const Trace>(outer);auto ff=fold(aa,b,"layer"+std::to_string(i),"nested");outer.scope=ff.scope;outer.lineage=ff.lineage;outer.parents={aa,b};outer.proposal=ff.digest;outer.closures[0].digest=hash(prefix(outer,3));validate(outer);}
 check(numeric("ratio",mpq_class(mpz_class(2),mpz_class(-4))).n==mpq_class(-1,2),"ratio normalization");
 check(binary("//",numeric("int",-7),numeric("int",3)).n==-3,"floor division");check(binary("%",numeric("int",-7),numeric("int",3)).n==2,"floor remainder");
 rejects([&]{binary("+",numeric("t32",mpq_class(mpz_class("926510094425920"))),numeric("t32",1));},"t32 overflow");
 void*c=dec_context_new(10,2);void*v=dec_literal(c,"str","a\0b",3,3);dec_emit(c,v,3);check(dec_output_size(c)==4,"NUL emit length");check(dec_render(c,v)[2]=='b'&&dec_render_size(c)==3,"NUL return length");
 void*other=dec_context_new(10,2);dec_unary(other,"-",v,7);check(!dec_ok(other)&&std::string(dec_error(other))=="line 7: invalid or foreign value handle"&&dec_error_line(other)==7,"cross-context ownership");dec_context_free(other);
 dec_set_allocation_limit(c,1);check(!dec_literal(c,"int","1",1,8)&&std::string(dec_error(c))=="line 8: runtime allocation limit exceeded","allocation failure");dec_context_free(c);
 c=dec_context_new(1,1);check(dec_tick(c)==1&&dec_tick(c)==0,"fuel boundary");dec_context_free(c);
 c=dec_context_new(10,1);check(dec_enter(c)==1&&dec_enter(c)==0,"depth boundary");dec_leave(c);dec_leave(c);dec_context_free(c);
 check(quote("é😀\n") == "\"\\u00e9\\ud83d\\ude00\\n\"","Unicode canonicalization");
 std::cout<<"PASS native runtime: exact arithmetic, preserved cancellation, certificate/scope/parent/event/replay mutations, nested retention, allocation, ownership, fuel, depth, Unicode/NUL\n";
 return 0;
}catch(const std::exception&e){std::cerr<<e.what()<<'\n';return 1;}}
