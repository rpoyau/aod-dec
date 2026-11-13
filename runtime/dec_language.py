"""Dec core frontend and deterministic IR evaluator.

This module parses a deliberately finite subset. It never uses eval or exec.
Every body is typechecked, including functions not reached by main.
"""
from __future__ import annotations
import ast
import hashlib
import json
import keyword
import re
from . import dec_core as core

FORMAT = "aod.dec.ir"
VERSION = 1
MAX_SOURCE = 1_000_000
MAX_IR = 8_000_000
IDENT = r"[A-Za-z_][A-Za-z_0-9]*"
NUMERIC = {"int", "t1", "t32", "ratio"}

class DecError(ValueError):
    pass

def fail(line, message):
    raise DecError(f"line {line}: {message}")

def _identifier(name):
    return bool(re.fullmatch(IDENT, name)) and not keyword.iskeyword(name) and name not in {"true", "false"}

def _comment(line):
    quote = None
    escaped = False
    for i, c in enumerate(line):
        if quote:
            if escaped: escaped = False
            elif c == "\\": escaped = True
            elif c == quote: quote = None
        elif c in "\"'": quote = c
        elif c == "#": return line[:i]
    return line

def _names(text, line):
    if not text.strip(): return []
    names = [v.strip() for v in text.split(",")]
    if any(not _identifier(v) for v in names) or len(set(names)) != len(names):
        fail(line, "expected distinct parameter identifiers")
    return names

def _parse(source):
    if not isinstance(source, str) or len(source.encode()) > MAX_SOURCE:
        raise DecError("source exceeds the 1 MB core limit")
    functions = []
    current = None
    branch = None
    branch_indent = body_indent = None
    for number, raw in enumerate(source.splitlines(), 1):
        if "\t" in raw[:len(raw)-len(raw.lstrip())]: fail(number, "tabs in indentation are unsupported")
        clean = _comment(raw).rstrip()
        if not clean.strip(): continue
        indent = len(clean)-len(clean.lstrip(" "))
        line = clean.strip()
        if indent == 0:
            m = re.fullmatch(rf"Dec\s+({IDENT})\s*\((.*?)\)\s*->\s*({IDENT})\s*:", line)
            if not m: fail(number, "expected Dec name(parameters) -> result:; unsupported top-level syntax")
            name, params, output = m.groups()
            if not _identifier(name) or not _identifier(output): fail(number, "reserved word cannot be a function or result binding")
            current = {"name":name, "params":_names(params,number), "output":output,"line":number,"branches":[]}
            if any(f["name"] == name for f in functions): fail(number, "put overloads in one Dec declaration")
            if name in core.BUILTIN_SIGNATURES: fail(number, f"cannot redefine builtin {name}")
            if output in current["params"]: fail(number, "result binding must differ from parameters")
            functions.append(current); branch = None; branch_indent = body_indent = None
        else:
            if current is None: fail(number, "indented content requires a Dec declaration")
            if branch_indent is None or indent == branch_indent:
                m = re.fullmatch(rf"\((.*?)\)\s*->\s*({IDENT})\s+({IDENT})\s*:", line)
                if not m: fail(number, "expected a typed signature row; literal patterns, tuples and structs are not in core v1")
                raw_params, result_type, result_name = m.groups()
                params=[]
                for part in raw_params.split(",") if raw_params.strip() else []:
                    p = re.fullmatch(rf"\s*({IDENT})\s+({IDENT})\s*",part)
                    if not p: fail(number, "signature parameters require an explicit type and name")
                    typ, name = p.groups()
                    if typ not in core.TYPES: fail(number, f"unknown type {typ}")
                    params.append({"type":typ,"name":name})
                if result_type not in core.TYPES: fail(number, f"unknown type {result_type}")
                if [p["name"] for p in params] != current["params"] or result_name != current["output"]:
                    fail(number, "signature bindings must match the Dec declaration")
                sig=tuple(p["type"] for p in params)
                if any(tuple(p["type"] for p in b["params"]) == sig for b in current["branches"]):
                    fail(number, "duplicate input signature; return type cannot resolve dispatch")
                branch={"params":params,"return_type":result_type,"output":result_name,"line":number,"body":[]}
                current["branches"].append(branch); branch_indent=indent; body_indent=None
            elif indent > branch_indent:
                if branch is None: fail(number,"body requires a signature")
                if body_indent is None: body_indent=indent
                if indent != body_indent: fail(number,"nested blocks are not in core v1")
                branch["body"].append((number,line))
            else: fail(number,"inconsistent indentation")
    if not functions: raise DecError("empty source: no Dec declarations")
    for f in functions:
        if not f["branches"]: fail(f["line"],"Dec declaration has no signature rows")
    return functions

def _signatures(functions):
    signatures={name:list(rows) for name,rows in core.BUILTIN_SIGNATURES.items()}
    for f in functions:
        signatures[f["name"]]=[(tuple(p["type"] for p in b["params"]),b["return_type"]) for b in f["branches"]]
    return signatures

def _expression(text, line, env, signatures):
    try: node=ast.parse(text,mode="eval").body
    except (SyntaxError, ValueError, RecursionError) as e: fail(line,f"invalid expression: {e}")
    def walk(n):
        if isinstance(n,ast.Constant):
            if type(n.value) is bool: typ="bool"
            elif type(n.value) is int: typ="int"
            elif type(n.value) is str: typ="str"
            else: fail(line,"only integer, Boolean and string literals are supported")
            return {"kind":"literal","type":typ,"value":n.value}
        if isinstance(n,ast.Name):
            if n.id in {"true","false"}: return {"kind":"literal","type":"bool","value":n.id=="true"}
            if n.id not in env: fail(line,f"undefined or uninitialized name {n.id}")
            return {"kind":"name","type":env[n.id],"name":n.id}
        if isinstance(n,ast.Call):
            if not isinstance(n.func,ast.Name) or n.keywords: fail(line,"calls require a direct name and positional arguments")
            name=n.func.id; args=[walk(a) for a in n.args]; sig=tuple(a["type"] for a in args)
            rows=signatures.get(name)
            if rows is None: fail(line,f"undefined function {name}")
            matches=[ret for inputs,ret in rows if tuple(inputs)==sig]
            if len(matches)!=1: fail(line,f"unlisted signature {name}({', '.join(sig)})")
            return {"kind":"call","type":matches[0],"name":name,"args":args}
        if isinstance(n,ast.BinOp):
            operators={ast.Add:"+",ast.Sub:"-",ast.Mult:"*",ast.Div:"/",ast.FloorDiv:"//",ast.Mod:"%"}
            op=operators.get(type(n.op))
            if op is None: fail(line,"unsupported operator")
            a,b=walk(n.left),walk(n.right); typ=a["type"]
            if typ != b["type"] or typ not in NUMERIC: fail(line,"arithmetic requires matching explicit numeric types")
            if op in {"//","%"} and typ not in {"int","t32"}: fail(line,"quotient/remainder require int or t32")
            return {"kind":"binary","type":"ratio" if op=="/" else typ,"op":op,"left":a,"right":b}
        if isinstance(n,ast.UnaryOp):
            operators={ast.USub:"-",ast.UAdd:"+",ast.Not:"not"}; op=operators.get(type(n.op)); a=walk(n.operand)
            if op is None or (op=="not" and a["type"]!="bool") or (op!="not" and a["type"] not in NUMERIC): fail(line,"invalid unary operation")
            return {"kind":"unary","type":a["type"],"op":op,"value":a}
        if isinstance(n,ast.Compare):
            if len(n.ops)!=1: fail(line,"chained comparisons are not in core v1")
            operators={ast.Eq:"==",ast.NotEq:"!=",ast.Lt:"<",ast.LtE:"<=",ast.Gt:">",ast.GtE:">="}
            op=operators.get(type(n.ops[0])); a,b=walk(n.left),walk(n.comparators[0])
            if op is None or a["type"]!=b["type"]: fail(line,"comparison requires matching types")
            if op not in {"==","!="} and a["type"] not in NUMERIC: fail(line,"ordering requires a numeric type")
            return {"kind":"compare","type":"bool","op":op,"left":a,"right":b}
        if isinstance(n,ast.BoolOp):
            op={ast.And:"and",ast.Or:"or"}.get(type(n.op)); vals=[walk(a) for a in n.values]
            if not op or any(a["type"]!="bool" for a in vals): fail(line,"Boolean operation requires bool operands")
            return {"kind":"boolean","type":"bool","op":op,"values":vals}
        if isinstance(n,ast.IfExp):
            cond,a,b=walk(n.test),walk(n.body),walk(n.orelse)
            if cond["type"]!="bool" or a["type"]!=b["type"]: fail(line,"conditional requires bool condition and matching result types")
            return {"kind":"conditional","type":a["type"],"condition":cond,"yes":a,"no":b}
        fail(line,f"unsupported expression {type(n).__name__}")
    try: return walk(node)
    except RecursionError: fail(line,"expression nesting exceeds compiler limit")

def compile_source(source):
    functions=_parse(source); signatures=_signatures(functions)
    for f in functions:
        for b in f["branches"]:
            env={p["name"]:p["type"] for p in b["params"]}; declared=dict(env); declared[b["output"]]=b["return_type"]
            body=[]; returned=False
            for line,text in b["body"]:
                if returned: fail(line,"unreachable statement after return")
                kind=None
                for word in ("assert","emit","return"):
                    if text.startswith(word+" "):
                        kind=word; expr=_expression(text[len(word)+1:].strip(),line,env,signatures)
                        if kind=="assert" and expr["type"]!="bool": fail(line,"assert requires bool")
                        if kind=="return" and expr["type"]!=b["return_type"]: fail(line,"return type mismatch")
                        body.append({"kind":kind,"value":expr,"line":line}); returned=kind=="return"; break
                if kind: continue
                m=re.fullmatch(rf"(?:(?P<type>{IDENT})\s+)?(?P<name>{IDENT})\s*=\s*(?!=)(?P<expr>.+)",text)
                if not m: fail(line,"unsupported statement; expected assignment, assert, emit or return")
                typ,name,value=m.group("type","name","expr")
                if not _identifier(name): fail(line,"reserved word cannot be a local binding")
                if typ:
                    if typ not in core.TYPES: fail(line,f"unknown type {typ}")
                    if name in declared: fail(line,f"duplicate local declaration {name}")
                    declared[name]=typ
                elif name not in declared: fail(line,f"declare local {name} with an explicit type")
                expr=_expression(value,line,env,signatures)
                if expr["type"]!=declared[name]: fail(line,f"assignment to {name} requires {declared[name]}, got {expr['type']}")
                env[name]=declared[name]; body.append({"kind":"assign","name":name,"type":declared[name],"value":expr,"line":line})
            if not returned and b["output"] not in env: fail(b["line"],"result is never assigned")
            b["body"]=body
    entries=[b for f in functions if f["name"]=="main" for b in f["branches"] if not b["params"]]
    if len(entries)!=1: raise DecError("core program requires exactly one main() signature")
    return {"format":FORMAT,"version":VERSION,"source_sha256":hashlib.sha256(source.encode()).hexdigest(),"source":source,"functions":functions}

def dumps_ir(program):
    return json.dumps(program,ensure_ascii=False,sort_keys=True,indent=2)+"\n"

def loads_ir(text):
    if len(text.encode())>MAX_IR: raise DecError("IR exceeds the 8 MB core limit")
    def pairs(values):
        d={}
        for k,v in values:
            if k in d: raise DecError(f"duplicate IR key {k}")
            d[k]=v
        return d
    try: program=json.loads(text,object_pairs_hook=pairs)
    except (ValueError,RecursionError) as e: raise DecError(f"invalid IR JSON: {e}") from e
    if not isinstance(program,dict) or program.get("format")!=FORMAT or type(program.get("version")) is not int or program.get("version")!=VERSION:
        raise DecError("unsupported Dec IR format/version")
    source=program.get("source")
    if not isinstance(source,str): raise DecError("IR source must be a string")
    rebuilt=compile_source(source)
    # Canonical JSON comparison prevents bool/int equality from accepting a changed typed value.
    if dumps_ir(program)!=dumps_ir(rebuilt): raise DecError("IR payload or source digest differs from validated source")
    return rebuilt

class Interpreter:
    def __init__(self,program,emit=print,max_steps=100000,max_depth=128):
        # Public API also revalidates, so callers cannot bypass the loader with a modified dict.
        self.program=loads_ir(dumps_ir(program)); self.emit=emit; self.remaining=max_steps; self.max_depth=max_depth; self.depth=0
        self.functions={f["name"]:f for f in self.program["functions"]}
    def tick(self):
        self.remaining-=1
        if self.remaining<0: raise DecError("execution step budget exhausted")
    def expr(self,e,env):
        self.tick(); kind=e["kind"]
        if kind=="literal": return e["value"]
        if kind=="name": return env[e["name"]]
        if kind=="call": return self.call(e["name"],[self.expr(a,env) for a in e["args"]])
        if kind=="binary": return core.apply_binary(e["op"],self.expr(e["left"],env),self.expr(e["right"],env))
        if kind=="unary":
            v=self.expr(e["value"],env)
            return not v if e["op"]=="not" else core.apply_unary(e["op"],v)
        if kind=="compare":
            a,b=self.expr(e["left"],env),self.expr(e["right"],env)
            if e["op"]=="==": return a==b
            if e["op"]=="!=": return a!=b
            return core.apply_binary(e["op"],a,b)
        if kind=="boolean":
            for a in e["values"]:
                value=self.expr(a,env)
                if e["op"]=="and" and not value: return False
                if e["op"]=="or" and value: return True
            return e["op"]=="and"
        if kind=="conditional": return self.expr(e["yes"] if self.expr(e["condition"],env) else e["no"],env)
        raise DecError("invalid internal expression")
    def call(self,name,args):
        self.tick()
        if name in core.BUILTIN_SIGNATURES: return core.call_builtin(name,args)
        self.depth+=1
        try:
            if self.depth>self.max_depth: raise DecError("call depth limit exceeded")
            sig=tuple(core.type_name(a) for a in args)
            branches=[b for b in self.functions[name]["branches"] if tuple(p["type"] for p in b["params"])==sig]
            if len(branches)!=1: raise DecError(f"unlisted runtime signature {name}{sig}")
            b=branches[0]; env={p["name"]:a for p,a in zip(b["params"],args)}
            for stmt in b["body"]:
                self.tick()
                try:
                    value=self.expr(stmt["value"],env)
                    if stmt["kind"]=="assign": env[stmt["name"]]=value
                    elif stmt["kind"]=="assert":
                        if not value: fail(stmt["line"],"assertion failed")
                    elif stmt["kind"]=="emit": self.emit(core.render(value))
                    elif stmt["kind"]=="return": return value
                except DecError: raise
                except (ValueError,TypeError,ArithmeticError) as e: fail(stmt["line"],str(e))
            return env[b["output"]]
        finally: self.depth-=1
    def run(self):
        try: return self.call("main",[])
        except RecursionError as e: raise DecError("interpreter nesting limit exceeded") from e

def run_source(source,emit=print,**kwargs):
    return Interpreter(compile_source(source),emit=emit,**kwargs).run()
