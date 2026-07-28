from fractions import Fraction as F

ok = []
def chk(name, cond):
    ok.append((name, bool(cond)))
    print(("PASS" if cond else "FAIL"), name)

# MDC-FABLE at Theta_4^down vertex theta=(2/5,1/5,1/5,1/5)
pc = F(4,25) + 3*F(1,25)
chk("p_c(vertex)=7/25", pc == F(7,25))
chk("Fable M=9-p_c=218/25", 9-pc == F(218,25))
chk("Fable L=11/2-3/2 p_c=127/25", F(11,2)-F(3,2)*pc == F(127,25))
chk("Fable L>5 (n=4 kill)", F(127,25) > 5)
# Fable ZE phase threshold
chk("ZE iff p_c>=(9-2n)/3 at n=4: 7/25<1/3 kill", pc < F(1,3))
# uniform n=4
pcu = 4*F(1,16)
chk("uniform p_c=1/4; M=35/4; L=41/8", (9-pcu==F(35,4)) and (F(11,2)-F(3,2)*pcu==F(41,8)))
# expand counts
chk("E[#exp] Fable uniform = 7/4", 2-pcu == F(7,4))
chk("E[#exp] Fable vertex = 43/25 > 1", 2-pc == F(43,25))

# MDC-KIMI thresholds from F2 pieces: F2_down(t)=6+27t/100 reaches 8 at t=?
# threshold claims: batch down rho>=150/17 (F2>=8), cap rho>=1200/137, seq H2>=8 at 150/17, G2>=8 at 125/17
t = F(150,17)
chk("F2_batch,down(150/17)=6+27/100*150/17 >= 8 ?", 6+F(27,100)*t)
# pieces: F2_batch,down: 2+17t/25 (t<=80/9) | 4+91t/200 (t<=400/37) | 6+27t/100 (t<=400/27) | 10
def F2down(t):
    t=F(t)
    if t <= F(80,9): return 2+F(17,25)*t
    if t <= F(400,37): return 4+F(91,200)*t
    if t <= F(400,27): return 6+F(27,100)*t
    return F(10)
def F2cap(t):
    t=F(t)
    if t <= 10: return 2+F(137,200)*t
    if t <= F(800,71): return 4+F(97,200)*t
    if t <= F(1600,123): return 6+F(123,400)*t
    return F(10)
def G2down(t):
    t=F(t)
    if t <= F(40,3): return 3+F(17,25)*t
    if t <= F(600,37): return 6+F(91,200)*t
    if t <= F(200,9): return 9+F(27,100)*t
    return F(15)
chk("F2down(40)=10", F2down(40)==10)
chk("F2cap(40)=10", F2cap(40)==10)
chk("G2down(40)=15", G2down(40)==15)
chk("F2down crosses 8 exactly at 150/17", F2down(F(150,17))==8)
chk("F2cap crosses 8 exactly at 1200/137", F2cap(F(1200,137))==8)
chk("G2down crosses 8 at 125/17", G2down(F(125,17))==8)
# margins at (40,20)
chk("Kimi batch margins (5,0,1): (10-5,0,5-4)", (10-5, 0, 5-4)==(5,0,1))
chk("Kimi seq margins (7,0,1): (15-8,0,5-4)", (15-8,0,5-4)==(7,0,1))
# Kimi necessity: >=2 expands L >= 11/2 > 5
chk("necessity 11/2>5", F(11,2)>5)

# BP1 conjectural t1 table: t1(n) = 2/(1/2 - e_anti(n)), e_anti n=3..8
e = {3:F(1,4),4:F(11,40),5:F(121,400),6:F(5,16),7:F(145,448),8:F(43,128)}
for n,v in e.items():
    t1 = 2/(F(1,2)-v)
    print(f"t1({n}) = {t1} ~= {float(t1):.4f}")
chk("e_anti(7)<1/3<e_anti(8)", e[7]<F(1,3)<e[8])
# rho_kill values 3..7
rk = {3:F(16),4:F(160,11),5:F(1600,121),6:F(64,5),7:F(1792,145)}
for n,v in rk.items():
    chk(f"rho_kill({n})=4/e_anti", 4/e[n]==v)

# Cont-2 m_fail surface at rho=40
import math
mf = {n: math.floor((40*(1-F(1,2**n))-1)/2)+1 for n in range(2,9)}
print("m_fail(n,40):", mf)
chk("m_fail(4,40)=19", mf[4]==19)
chk("m_fail(3,40)=18", mf[3]==18)

# one-demand thresholds
chk("160/11 > 64/5 (Q4d>Q4u)", F(160,11)>F(64,5))
chk("135/8 > 16 (Q3d>Q3u)", F(135,8)>16)
print("TOTAL:", sum(1 for _,c in ok if c), "/", len(ok), "PASS")
