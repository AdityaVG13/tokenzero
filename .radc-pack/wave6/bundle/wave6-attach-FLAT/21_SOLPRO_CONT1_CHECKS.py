from fractions import Fraction
from math import comb, log2

def p_uniform(m):
    return sum(Fraction(comb(4,k),16)*Fraction(k,4)**m for k in range(5))

def p_down(m):
    s=Fraction(0)
    for k in range(4):
        c=comb(3,k)
        s += Fraction(c,16)*(Fraction(k,5)**m + Fraction(k+2,5)**m)
    return s

def p_cap(m):
    s=Fraction(0)
    for a in range(3):
      for b in range(3):
        s += Fraction(comb(2,a)*comb(2,b),16)*Fraction(3*a+2*b,10)**m
    return s

def gap(p,m):
    return Fraction(39-2*m)-40*p

for name,fn in [('uniform',p_uniform),('down',p_down),('cap',p_cap)]:
    print(name)
    for m in (9,10,18,19):
        p=fn(m); g=gap(p,m)
        print(m,'P=',p,'gap=',g,'float=',float(g))
    assert gap(fn(18),18)>0
    assert gap(fn(19),19)<0

# General-demand RD numerical KKT consistency.
def H(x):
    if x in (0.0,1.0): return 0.0
    return -x*log2(x)-(1-x)*log2(1-x)
def f(x): return 1-H(x)
def point(theta,mu):
    ds=[1/(1+2**(mu*t)) for t in theta]
    D=sum(t*d for t,d in zip(theta,ds))
    R=sum(f(d) for d in ds)
    Rag=f(D)
    return D,R,Rag,ds
for theta in ([.25]*4,[.4,.2,.2,.2],[.3,.3,.2,.2]):
    prev=1e9
    for mu in (0.1,0.5,1,2,5,10,20):
        D,R,Rag,ds=point(theta,mu)
        G=R-Rag
        assert G>0
        assert G>0
        prev=G
    print('RD',theta,'ok')
