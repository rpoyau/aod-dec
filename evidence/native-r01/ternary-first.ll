; Dec semantic IR v1; executable operations, no source VM

declare i32 @dec_tick(ptr)
declare i32 @dec_enter(ptr)
declare void @dec_leave(ptr)
declare i32 @dec_ok(ptr)
declare ptr @dec_literal(ptr, ptr, ptr, i64, i32)
declare ptr @dec_unary(ptr, ptr, ptr, i32)
declare ptr @dec_binary(ptr, ptr, ptr, ptr, i32)
declare ptr @dec_builtin(ptr, ptr, ptr, i32, i32)
declare i32 @dec_truth(ptr, ptr)
declare void @dec_emit(ptr, ptr, i32)
declare void @dec_assert(ptr, ptr, i32)

@dec_s0 = private unnamed_addr constant [2 x i8] c"\2B\00"
@dec_s1 = private unnamed_addr constant [4 x i8] c"\69\6E\74\00"
@dec_s2 = private unnamed_addr constant [2 x i8] c"\31\00"
@dec_s3 = private unnamed_addr constant [4 x i8] c"\74\33\32\00"
@dec_s4 = private unnamed_addr constant [2 x i8] c"\32\00"
@dec_s5 = private unnamed_addr constant [3 x i8] c"\3D\3D\00"
@dec_s6 = private unnamed_addr constant [5 x i8] c"\70\61\63\6B\00"
@dec_s7 = private unnamed_addr constant [7 x i8] c"\75\6E\70\61\63\6B\00"
@dec_s8 = private unnamed_addr constant [11 x i8] c"\34\30\30\30\30\30\30\30\30\30\00"
@dec_s9 = private unnamed_addr constant [6 x i8] c"\72\61\74\69\6F\00"
@dec_s10 = private unnamed_addr constant [11 x i8] c"\32\30\30\30\30\30\30\30\30\30\00"
@dec_s11 = private unnamed_addr constant [2 x i8] c"\30\00"
define ptr @dec_add__t32_t32(ptr %ctx, ptr %arg0, ptr %arg1) {
entry:
  %v1 = call i32 @dec_tick(ptr %ctx)
  %v2 = icmp ne i32 %v1, 0
  br i1 %v2, label %enter, label %prefailed
enter:
  %v3 = call i32 @dec_enter(ptr %ctx)
  %v4 = icmp ne i32 %v3, 0
  br i1 %v4, label %ok5, label %failed
ok5:
  %v6 = call i32 @dec_tick(ptr %ctx)
  %v7 = icmp ne i32 %v6, 0
  br i1 %v7, label %ok8, label %failed
ok8:
  %v9 = call i32 @dec_tick(ptr %ctx)
  %v10 = icmp ne i32 %v9, 0
  br i1 %v10, label %ok11, label %failed
ok11:
  %v12 = call i32 @dec_tick(ptr %ctx)
  %v13 = icmp ne i32 %v12, 0
  br i1 %v13, label %ok14, label %failed
ok14:
  %v15 = call i32 @dec_tick(ptr %ctx)
  %v16 = icmp ne i32 %v15, 0
  br i1 %v16, label %ok17, label %failed
ok17:
  %v18 = call ptr @dec_binary(ptr %ctx, ptr @dec_s0, ptr %arg0, ptr %arg1, i32 4)
  %v19 = call i32 @dec_ok(ptr %ctx)
  %v20 = icmp ne i32 %v19, 0
  br i1 %v20, label %ok21, label %failed
ok21:
  call void @dec_leave(ptr %ctx)
  ret ptr %v18
failed:
  call void @dec_leave(ptr %ctx)
  ret ptr null
prefailed:
  ret ptr null
}
define ptr @dec_add__ratio_ratio(ptr %ctx, ptr %arg0, ptr %arg1) {
entry:
  %v1 = call i32 @dec_tick(ptr %ctx)
  %v2 = icmp ne i32 %v1, 0
  br i1 %v2, label %enter, label %prefailed
enter:
  %v3 = call i32 @dec_enter(ptr %ctx)
  %v4 = icmp ne i32 %v3, 0
  br i1 %v4, label %ok5, label %failed
ok5:
  %v6 = call i32 @dec_tick(ptr %ctx)
  %v7 = icmp ne i32 %v6, 0
  br i1 %v7, label %ok8, label %failed
ok8:
  %v9 = call i32 @dec_tick(ptr %ctx)
  %v10 = icmp ne i32 %v9, 0
  br i1 %v10, label %ok11, label %failed
ok11:
  %v12 = call i32 @dec_tick(ptr %ctx)
  %v13 = icmp ne i32 %v12, 0
  br i1 %v13, label %ok14, label %failed
ok14:
  %v15 = call i32 @dec_tick(ptr %ctx)
  %v16 = icmp ne i32 %v15, 0
  br i1 %v16, label %ok17, label %failed
ok17:
  %v18 = call ptr @dec_binary(ptr %ctx, ptr @dec_s0, ptr %arg0, ptr %arg1, i32 6)
  %v19 = call i32 @dec_ok(ptr %ctx)
  %v20 = icmp ne i32 %v19, 0
  br i1 %v20, label %ok21, label %failed
ok21:
  call void @dec_leave(ptr %ctx)
  ret ptr %v18
failed:
  call void @dec_leave(ptr %ctx)
  ret ptr null
prefailed:
  ret ptr null
}
define ptr @dec_main__void(ptr %ctx) {
entry:
  %v1 = call i32 @dec_tick(ptr %ctx)
  %v2 = icmp ne i32 %v1, 0
  br i1 %v2, label %enter, label %prefailed
enter:
  %v3 = call i32 @dec_enter(ptr %ctx)
  %v4 = icmp ne i32 %v3, 0
  br i1 %v4, label %ok5, label %failed
ok5:
  %v6 = call i32 @dec_tick(ptr %ctx)
  %v7 = icmp ne i32 %v6, 0
  br i1 %v7, label %ok8, label %failed
ok8:
  %v9 = call i32 @dec_tick(ptr %ctx)
  %v10 = icmp ne i32 %v9, 0
  br i1 %v10, label %ok11, label %failed
ok11:
  %v12 = call i32 @dec_tick(ptr %ctx)
  %v13 = icmp ne i32 %v12, 0
  br i1 %v13, label %ok14, label %failed
ok14:
  %v15 = call i32 @dec_tick(ptr %ctx)
  %v16 = icmp ne i32 %v15, 0
  br i1 %v16, label %ok17, label %failed
ok17:
  %v18 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s2, i64 1, i32 10)
  %v19 = call i32 @dec_ok(ptr %ctx)
  %v20 = icmp ne i32 %v19, 0
  br i1 %v20, label %ok21, label %failed
ok21:
  %v22 = call i32 @dec_tick(ptr %ctx)
  %v23 = icmp ne i32 %v22, 0
  br i1 %v23, label %ok24, label %failed
ok24:
  %v25 = alloca [1 x ptr]
  %v26 = getelementptr [1 x ptr], ptr %v25, i64 0, i64 0
  store ptr %v18, ptr %v26
  %v27 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s3, ptr %v25, i32 1, i32 10)
  %v28 = call i32 @dec_ok(ptr %ctx)
  %v29 = icmp ne i32 %v28, 0
  br i1 %v29, label %ok30, label %failed
ok30:
  %v31 = call i32 @dec_tick(ptr %ctx)
  %v32 = icmp ne i32 %v31, 0
  br i1 %v32, label %ok33, label %failed
ok33:
  %v34 = call i32 @dec_tick(ptr %ctx)
  %v35 = icmp ne i32 %v34, 0
  br i1 %v35, label %ok36, label %failed
ok36:
  %v37 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s2, i64 1, i32 10)
  %v38 = call i32 @dec_ok(ptr %ctx)
  %v39 = icmp ne i32 %v38, 0
  br i1 %v39, label %ok40, label %failed
ok40:
  %v41 = call i32 @dec_tick(ptr %ctx)
  %v42 = icmp ne i32 %v41, 0
  br i1 %v42, label %ok43, label %failed
ok43:
  %v44 = alloca [1 x ptr]
  %v45 = getelementptr [1 x ptr], ptr %v44, i64 0, i64 0
  store ptr %v37, ptr %v45
  %v46 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s3, ptr %v44, i32 1, i32 10)
  %v47 = call i32 @dec_ok(ptr %ctx)
  %v48 = icmp ne i32 %v47, 0
  br i1 %v48, label %ok49, label %failed
ok49:
  %v50 = call ptr @dec_add__t32_t32(ptr %ctx, ptr %v27, ptr %v46)
  %v51 = call i32 @dec_ok(ptr %ctx)
  %v52 = icmp ne i32 %v51, 0
  br i1 %v52, label %ok53, label %failed
ok53:
  %v54 = call i32 @dec_tick(ptr %ctx)
  %v55 = icmp ne i32 %v54, 0
  br i1 %v55, label %ok56, label %failed
ok56:
  %v57 = call i32 @dec_tick(ptr %ctx)
  %v58 = icmp ne i32 %v57, 0
  br i1 %v58, label %ok59, label %failed
ok59:
  %v60 = call i32 @dec_tick(ptr %ctx)
  %v61 = icmp ne i32 %v60, 0
  br i1 %v61, label %ok62, label %failed
ok62:
  %v63 = call i32 @dec_tick(ptr %ctx)
  %v64 = icmp ne i32 %v63, 0
  br i1 %v64, label %ok65, label %failed
ok65:
  %v66 = call i32 @dec_tick(ptr %ctx)
  %v67 = icmp ne i32 %v66, 0
  br i1 %v67, label %ok68, label %failed
ok68:
  %v69 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s4, i64 1, i32 11)
  %v70 = call i32 @dec_ok(ptr %ctx)
  %v71 = icmp ne i32 %v70, 0
  br i1 %v71, label %ok72, label %failed
ok72:
  %v73 = call i32 @dec_tick(ptr %ctx)
  %v74 = icmp ne i32 %v73, 0
  br i1 %v74, label %ok75, label %failed
ok75:
  %v76 = alloca [1 x ptr]
  %v77 = getelementptr [1 x ptr], ptr %v76, i64 0, i64 0
  store ptr %v69, ptr %v77
  %v78 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s3, ptr %v76, i32 1, i32 11)
  %v79 = call i32 @dec_ok(ptr %ctx)
  %v80 = icmp ne i32 %v79, 0
  br i1 %v80, label %ok81, label %failed
ok81:
  %v82 = call ptr @dec_binary(ptr %ctx, ptr @dec_s5, ptr %v50, ptr %v78, i32 11)
  %v83 = call i32 @dec_ok(ptr %ctx)
  %v84 = icmp ne i32 %v83, 0
  br i1 %v84, label %ok85, label %failed
ok85:
  call void @dec_assert(ptr %ctx, ptr %v82, i32 11)
  %v86 = call i32 @dec_ok(ptr %ctx)
  %v87 = icmp ne i32 %v86, 0
  br i1 %v87, label %ok88, label %failed
ok88:
  %v89 = call i32 @dec_tick(ptr %ctx)
  %v90 = icmp ne i32 %v89, 0
  br i1 %v90, label %ok91, label %failed
ok91:
  %v92 = call i32 @dec_tick(ptr %ctx)
  %v93 = icmp ne i32 %v92, 0
  br i1 %v93, label %ok94, label %failed
ok94:
  %v95 = call i32 @dec_tick(ptr %ctx)
  %v96 = icmp ne i32 %v95, 0
  br i1 %v96, label %ok97, label %failed
ok97:
  %v98 = call i32 @dec_tick(ptr %ctx)
  %v99 = icmp ne i32 %v98, 0
  br i1 %v99, label %ok100, label %failed
ok100:
  %v101 = call i32 @dec_tick(ptr %ctx)
  %v102 = icmp ne i32 %v101, 0
  br i1 %v102, label %ok103, label %failed
ok103:
  %v104 = call i32 @dec_tick(ptr %ctx)
  %v105 = icmp ne i32 %v104, 0
  br i1 %v105, label %ok106, label %failed
ok106:
  %v107 = alloca [1 x ptr]
  %v108 = getelementptr [1 x ptr], ptr %v107, i64 0, i64 0
  store ptr %v50, ptr %v108
  %v109 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s6, ptr %v107, i32 1, i32 12)
  %v110 = call i32 @dec_ok(ptr %ctx)
  %v111 = icmp ne i32 %v110, 0
  br i1 %v111, label %ok112, label %failed
ok112:
  %v113 = call i32 @dec_tick(ptr %ctx)
  %v114 = icmp ne i32 %v113, 0
  br i1 %v114, label %ok115, label %failed
ok115:
  %v116 = alloca [1 x ptr]
  %v117 = getelementptr [1 x ptr], ptr %v116, i64 0, i64 0
  store ptr %v109, ptr %v117
  %v118 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s7, ptr %v116, i32 1, i32 12)
  %v119 = call i32 @dec_ok(ptr %ctx)
  %v120 = icmp ne i32 %v119, 0
  br i1 %v120, label %ok121, label %failed
ok121:
  %v122 = call i32 @dec_tick(ptr %ctx)
  %v123 = icmp ne i32 %v122, 0
  br i1 %v123, label %ok124, label %failed
ok124:
  %v125 = call ptr @dec_binary(ptr %ctx, ptr @dec_s5, ptr %v118, ptr %v50, i32 12)
  %v126 = call i32 @dec_ok(ptr %ctx)
  %v127 = icmp ne i32 %v126, 0
  br i1 %v127, label %ok128, label %failed
ok128:
  call void @dec_assert(ptr %ctx, ptr %v125, i32 12)
  %v129 = call i32 @dec_ok(ptr %ctx)
  %v130 = icmp ne i32 %v129, 0
  br i1 %v130, label %ok131, label %failed
ok131:
  %v132 = call i32 @dec_tick(ptr %ctx)
  %v133 = icmp ne i32 %v132, 0
  br i1 %v133, label %ok134, label %failed
ok134:
  %v135 = call i32 @dec_tick(ptr %ctx)
  %v136 = icmp ne i32 %v135, 0
  br i1 %v136, label %ok137, label %failed
ok137:
  call void @dec_emit(ptr %ctx, ptr %v50, i32 13)
  %v138 = call i32 @dec_ok(ptr %ctx)
  %v139 = icmp ne i32 %v138, 0
  br i1 %v139, label %ok140, label %failed
ok140:
  %v141 = call i32 @dec_tick(ptr %ctx)
  %v142 = icmp ne i32 %v141, 0
  br i1 %v142, label %ok143, label %failed
ok143:
  %v144 = call i32 @dec_tick(ptr %ctx)
  %v145 = icmp ne i32 %v144, 0
  br i1 %v145, label %ok146, label %failed
ok146:
  %v147 = call i32 @dec_tick(ptr %ctx)
  %v148 = icmp ne i32 %v147, 0
  br i1 %v148, label %ok149, label %failed
ok149:
  %v150 = call i32 @dec_tick(ptr %ctx)
  %v151 = icmp ne i32 %v150, 0
  br i1 %v151, label %ok152, label %failed
ok152:
  %v153 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s2, i64 1, i32 14)
  %v154 = call i32 @dec_ok(ptr %ctx)
  %v155 = icmp ne i32 %v154, 0
  br i1 %v155, label %ok156, label %failed
ok156:
  %v157 = call i32 @dec_tick(ptr %ctx)
  %v158 = icmp ne i32 %v157, 0
  br i1 %v158, label %ok159, label %failed
ok159:
  %v160 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s8, i64 10, i32 14)
  %v161 = call i32 @dec_ok(ptr %ctx)
  %v162 = icmp ne i32 %v161, 0
  br i1 %v162, label %ok163, label %failed
ok163:
  %v164 = call i32 @dec_tick(ptr %ctx)
  %v165 = icmp ne i32 %v164, 0
  br i1 %v165, label %ok166, label %failed
ok166:
  %v167 = alloca [2 x ptr]
  %v168 = getelementptr [2 x ptr], ptr %v167, i64 0, i64 0
  store ptr %v153, ptr %v168
  %v169 = getelementptr [2 x ptr], ptr %v167, i64 0, i64 1
  store ptr %v160, ptr %v169
  %v170 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s9, ptr %v167, i32 2, i32 14)
  %v171 = call i32 @dec_ok(ptr %ctx)
  %v172 = icmp ne i32 %v171, 0
  br i1 %v172, label %ok173, label %failed
ok173:
  %v174 = call i32 @dec_tick(ptr %ctx)
  %v175 = icmp ne i32 %v174, 0
  br i1 %v175, label %ok176, label %failed
ok176:
  %v177 = call i32 @dec_tick(ptr %ctx)
  %v178 = icmp ne i32 %v177, 0
  br i1 %v178, label %ok179, label %failed
ok179:
  %v180 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s2, i64 1, i32 14)
  %v181 = call i32 @dec_ok(ptr %ctx)
  %v182 = icmp ne i32 %v181, 0
  br i1 %v182, label %ok183, label %failed
ok183:
  %v184 = call i32 @dec_tick(ptr %ctx)
  %v185 = icmp ne i32 %v184, 0
  br i1 %v185, label %ok186, label %failed
ok186:
  %v187 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s8, i64 10, i32 14)
  %v188 = call i32 @dec_ok(ptr %ctx)
  %v189 = icmp ne i32 %v188, 0
  br i1 %v189, label %ok190, label %failed
ok190:
  %v191 = call i32 @dec_tick(ptr %ctx)
  %v192 = icmp ne i32 %v191, 0
  br i1 %v192, label %ok193, label %failed
ok193:
  %v194 = alloca [2 x ptr]
  %v195 = getelementptr [2 x ptr], ptr %v194, i64 0, i64 0
  store ptr %v180, ptr %v195
  %v196 = getelementptr [2 x ptr], ptr %v194, i64 0, i64 1
  store ptr %v187, ptr %v196
  %v197 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s9, ptr %v194, i32 2, i32 14)
  %v198 = call i32 @dec_ok(ptr %ctx)
  %v199 = icmp ne i32 %v198, 0
  br i1 %v199, label %ok200, label %failed
ok200:
  %v201 = call ptr @dec_add__ratio_ratio(ptr %ctx, ptr %v170, ptr %v197)
  %v202 = call i32 @dec_ok(ptr %ctx)
  %v203 = icmp ne i32 %v202, 0
  br i1 %v203, label %ok204, label %failed
ok204:
  %v205 = call i32 @dec_tick(ptr %ctx)
  %v206 = icmp ne i32 %v205, 0
  br i1 %v206, label %ok207, label %failed
ok207:
  %v208 = call i32 @dec_tick(ptr %ctx)
  %v209 = icmp ne i32 %v208, 0
  br i1 %v209, label %ok210, label %failed
ok210:
  %v211 = call i32 @dec_tick(ptr %ctx)
  %v212 = icmp ne i32 %v211, 0
  br i1 %v212, label %ok213, label %failed
ok213:
  %v214 = call i32 @dec_tick(ptr %ctx)
  %v215 = icmp ne i32 %v214, 0
  br i1 %v215, label %ok216, label %failed
ok216:
  %v217 = call i32 @dec_tick(ptr %ctx)
  %v218 = icmp ne i32 %v217, 0
  br i1 %v218, label %ok219, label %failed
ok219:
  %v220 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s2, i64 1, i32 15)
  %v221 = call i32 @dec_ok(ptr %ctx)
  %v222 = icmp ne i32 %v221, 0
  br i1 %v222, label %ok223, label %failed
ok223:
  %v224 = call i32 @dec_tick(ptr %ctx)
  %v225 = icmp ne i32 %v224, 0
  br i1 %v225, label %ok226, label %failed
ok226:
  %v227 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s10, i64 10, i32 15)
  %v228 = call i32 @dec_ok(ptr %ctx)
  %v229 = icmp ne i32 %v228, 0
  br i1 %v229, label %ok230, label %failed
ok230:
  %v231 = call i32 @dec_tick(ptr %ctx)
  %v232 = icmp ne i32 %v231, 0
  br i1 %v232, label %ok233, label %failed
ok233:
  %v234 = alloca [2 x ptr]
  %v235 = getelementptr [2 x ptr], ptr %v234, i64 0, i64 0
  store ptr %v220, ptr %v235
  %v236 = getelementptr [2 x ptr], ptr %v234, i64 0, i64 1
  store ptr %v227, ptr %v236
  %v237 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s9, ptr %v234, i32 2, i32 15)
  %v238 = call i32 @dec_ok(ptr %ctx)
  %v239 = icmp ne i32 %v238, 0
  br i1 %v239, label %ok240, label %failed
ok240:
  %v241 = call ptr @dec_binary(ptr %ctx, ptr @dec_s5, ptr %v201, ptr %v237, i32 15)
  %v242 = call i32 @dec_ok(ptr %ctx)
  %v243 = icmp ne i32 %v242, 0
  br i1 %v243, label %ok244, label %failed
ok244:
  call void @dec_assert(ptr %ctx, ptr %v241, i32 15)
  %v245 = call i32 @dec_ok(ptr %ctx)
  %v246 = icmp ne i32 %v245, 0
  br i1 %v246, label %ok247, label %failed
ok247:
  %v248 = call i32 @dec_tick(ptr %ctx)
  %v249 = icmp ne i32 %v248, 0
  br i1 %v249, label %ok250, label %failed
ok250:
  %v251 = call i32 @dec_tick(ptr %ctx)
  %v252 = icmp ne i32 %v251, 0
  br i1 %v252, label %ok253, label %failed
ok253:
  call void @dec_emit(ptr %ctx, ptr %v201, i32 16)
  %v254 = call i32 @dec_ok(ptr %ctx)
  %v255 = icmp ne i32 %v254, 0
  br i1 %v255, label %ok256, label %failed
ok256:
  %v257 = call i32 @dec_tick(ptr %ctx)
  %v258 = icmp ne i32 %v257, 0
  br i1 %v258, label %ok259, label %failed
ok259:
  %v260 = call i32 @dec_tick(ptr %ctx)
  %v261 = icmp ne i32 %v260, 0
  br i1 %v261, label %ok262, label %failed
ok262:
  %v263 = call i32 @dec_tick(ptr %ctx)
  %v264 = icmp ne i32 %v263, 0
  br i1 %v264, label %ok265, label %failed
ok265:
  %v266 = call ptr @dec_literal(ptr %ctx, ptr @dec_s1, ptr @dec_s11, i64 1, i32 17)
  %v267 = call i32 @dec_ok(ptr %ctx)
  %v268 = icmp ne i32 %v267, 0
  br i1 %v268, label %ok269, label %failed
ok269:
  %v270 = call i32 @dec_tick(ptr %ctx)
  %v271 = icmp ne i32 %v270, 0
  br i1 %v271, label %ok272, label %failed
ok272:
  %v273 = alloca [1 x ptr]
  %v274 = getelementptr [1 x ptr], ptr %v273, i64 0, i64 0
  store ptr %v266, ptr %v274
  %v275 = call ptr @dec_builtin(ptr %ctx, ptr @dec_s3, ptr %v273, i32 1, i32 17)
  %v276 = call i32 @dec_ok(ptr %ctx)
  %v277 = icmp ne i32 %v276, 0
  br i1 %v277, label %ok278, label %failed
ok278:
  call void @dec_leave(ptr %ctx)
  ret ptr %v275
failed:
  call void @dec_leave(ptr %ctx)
  ret ptr null
prefailed:
  ret ptr null
}
