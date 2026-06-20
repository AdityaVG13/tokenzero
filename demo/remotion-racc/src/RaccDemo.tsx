import React from 'react';
import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';

const FPS = 30;

const c = {
  bg: '#080b11',
  surface: '#0d121b',
  surfaceHi: '#111826',
  line: '#1d2735',
  lineHi: '#2a3850',
  ink: '#eef3fa',
  muted: '#8696ad',
  faint: '#56657c',
  emerald: '#34d399',
  sky: '#56b6f7',
  rose: '#fb7185',
  gold: '#f5c451',
};

const mono = 'SFMono-Regular, "JetBrains Mono", Menlo, Consolas, monospace';
const sans = '"Inter", "SF Pro Display", -apple-system, "Segoe UI", system-ui, sans-serif';

const scenes = [
  {from: 0, duration: 150},
  {from: 150, duration: 210},
  {from: 360, duration: 240},
  {from: 600, duration: 240},
  {from: 840, duration: 240},
  {from: 1080, duration: 210},
  {from: 1290, duration: 150},
];

const clamp = (v: number) => Math.max(0, Math.min(1, v));
const localFrame = (frame: number, scene: number) => frame - scenes[scene].from;

const ease = (frame: number, a: number, b: number) =>
  interpolate(frame, [a, b], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  });

const fade = (frame: number, scene: number) => {
  const {from, duration} = scenes[scene];
  const fadeIn = interpolate(frame, [from, from + 16], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  const fadeOut = interpolate(frame, [from + duration - 16, from + duration], [1, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
  });
  return Math.min(fadeIn, fadeOut);
};

const rise = (frame: number, delay = 0) =>
  spring({frame: Math.max(0, frame - delay), fps: FPS, config: {damping: 200, stiffness: 120, mass: 0.9}});

const countUp = (frame: number, target: number, a: number, b: number) =>
  Math.round(interpolate(frame, [a, b], [0, target], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  }));

const fmt = (n: number) => n.toLocaleString('en-US');

const Eyebrow: React.FC<{children: React.ReactNode; color?: string}> = ({children, color = c.emerald}) => (
  <div
    style={{
      fontFamily: mono,
      fontSize: 21,
      letterSpacing: 6,
      textTransform: 'uppercase',
      color,
      display: 'flex',
      alignItems: 'center',
      gap: 16,
    }}
  >
    <span style={{width: 34, height: 2, background: color, display: 'inline-block'}} />
    {children}
  </div>
);

const Display: React.FC<{children: React.ReactNode; size?: number}> = ({children, size = 92}) => (
  <div
    style={{
      fontFamily: sans,
      fontSize: size,
      fontWeight: 800,
      letterSpacing: -3,
      lineHeight: 1.02,
      color: c.ink,
    }}
  >
    {children}
  </div>
);

const Lede: React.FC<{children: React.ReactNode; width?: number}> = ({children, width = 640}) => (
  <div style={{fontFamily: sans, fontSize: 28, lineHeight: 1.4, color: c.muted, maxWidth: width}}>
    {children}
  </div>
);

const Terminal: React.FC<{lines: {t: string; c?: string}[]; width?: number; reveal?: number}> = ({
  lines,
  width = 760,
  reveal = 1,
}) => {
  const shown = Math.ceil(lines.length * reveal);
  return (
    <div
      style={{
        width,
        background: '#05080d',
        border: `1px solid ${c.lineHi}`,
        borderRadius: 16,
        overflow: 'hidden',
        boxShadow: '0 30px 80px rgba(0,0,0,0.5)',
      }}
    >
      <div
        style={{
          height: 44,
          background: c.surface,
          borderBottom: `1px solid ${c.line}`,
          display: 'flex',
          alignItems: 'center',
          padding: '0 18px',
          gap: 9,
        }}
      >
        <span style={{width: 12, height: 12, borderRadius: 99, background: '#ff5f57'}} />
        <span style={{width: 12, height: 12, borderRadius: 99, background: '#febc2e'}} />
        <span style={{width: 12, height: 12, borderRadius: 99, background: '#28c840'}} />
        <span style={{marginLeft: 14, fontFamily: mono, fontSize: 16, color: c.faint, letterSpacing: 1}}>
          tokenzero
        </span>
      </div>
      <div style={{padding: '26px 30px', fontFamily: mono, fontSize: 24, lineHeight: 1.7}}>
        {lines.slice(0, shown).map((l, i) => (
          <div key={i} style={{color: l.c ?? c.ink, whiteSpace: 'pre'}}>
            {l.t.startsWith('$') ? (
              <>
                <span style={{color: c.emerald}}>$</span>
                <span>{l.t.slice(1)}</span>
              </>
            ) : (
              l.t
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

const Grid: React.FC = () => (
  <AbsoluteFill
    style={{
      backgroundImage: `linear-gradient(${c.line}33 1px, transparent 1px), linear-gradient(90deg, ${c.line}33 1px, transparent 1px)`,
      backgroundSize: '64px 64px',
      maskImage: 'radial-gradient(ellipse 70% 60% at 50% 45%, black 30%, transparent 80%)',
      WebkitMaskImage: 'radial-gradient(ellipse 70% 60% at 50% 45%, black 30%, transparent 80%)',
    }}
  />
);

const SceneFrame: React.FC<{scene: number; children: React.ReactNode}> = ({scene, children}) => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{opacity: fade(frame, scene), background: c.bg, fontFamily: sans}}>
      <Grid />
      <AbsoluteFill
        style={{
          background: 'radial-gradient(ellipse 90% 70% at 50% 50%, transparent 55%, rgba(0,0,0,0.55) 100%)',
        }}
      />
      {children}
      <div
        style={{
          position: 'absolute',
          bottom: 44,
          left: 96,
          right: 96,
          display: 'flex',
          justifyContent: 'space-between',
          fontFamily: mono,
          fontSize: 18,
          letterSpacing: 2,
          color: c.faint,
        }}
      >
        <span>TOKENZERO</span>
        <span>RECOVERY-AWARE CONTEXT COMPRESSION</span>
        <span>{String(scene + 1).padStart(2, '0')} / 07</span>
      </div>
    </AbsoluteFill>
  );
};

const TitleScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 0);
  const r = rise(f, 4);
  return (
    <SceneFrame scene={0}>
      <AbsoluteFill style={{justifyContent: 'center', paddingLeft: 150}}>
        <div style={{opacity: ease(f, 0, 16), transform: `translateY(${(1 - r) * 22}px)`}}>
          <Eyebrow>How RACC works</Eyebrow>
          <div style={{marginTop: 30}}>
            <Display size={138}>Compress hard.</Display>
            <Display size={138}>
              <span style={{color: c.emerald}}>Recover</span> exact.
            </Display>
            <Display size={138}>Measure honest.</Display>
          </div>
          <div style={{marginTop: 46, opacity: ease(f, 22, 44)}}>
            <Lede width={760}>
              Recovery-Aware Context Compression hides tool output behind addressable refs, then proves
              every hidden byte is one expand away.
            </Lede>
          </div>
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

const Bar: React.FC<{
  label: string;
  value: number;
  max: number;
  color: string;
  frame: number;
  delay: number;
}> = ({label, value, max, color, frame, delay}) => {
  const g = ease(frame, delay, delay + 46);
  return (
    <div style={{display: 'flex', alignItems: 'center', gap: 26}}>
      <div style={{width: 168, textAlign: 'right', fontFamily: sans, fontSize: 26, color: c.muted}}>
        {label}
      </div>
      <div style={{flex: 1, height: 56, background: c.surface, borderRadius: 10, position: 'relative'}}>
        <div
          style={{
            position: 'absolute',
            inset: 0,
            width: `${(value / max) * 100 * g}%`,
            background: `linear-gradient(90deg, ${color}, ${color}bb)`,
            borderRadius: 10,
            boxShadow: `0 0 30px ${color}44`,
          }}
        />
      </div>
      <div
        style={{
          width: 220,
          fontFamily: mono,
          fontSize: 38,
          fontWeight: 700,
          color,
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {fmt(countUp(frame, value, delay, delay + 46))}
      </div>
    </div>
  );
};

const ProblemScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 1);
  return (
    <SceneFrame scene={1}>
      <AbsoluteFill style={{flexDirection: 'column', justifyContent: 'center', padding: '0 96px'}}>
        <div style={{opacity: ease(f, 0, 14)}}>
          <Eyebrow color={c.rose}>The problem</Eyebrow>
          <div style={{marginTop: 22, display: 'flex', alignItems: 'flex-end', justifyContent: 'space-between'}}>
            <Display size={84}>Tool output is context debt.</Display>
            <div style={{textAlign: 'right'}}>
              <div style={{fontFamily: mono, fontSize: 24, color: c.faint, letterSpacing: 3}}>SAVINGS</div>
              <div style={{fontFamily: sans, fontSize: 110, fontWeight: 800, color: c.emerald, letterSpacing: -4}}>
                {countUp(f, 991, 60, 130) / 10}%
              </div>
            </div>
          </div>
        </div>
        <div style={{marginTop: 70, display: 'flex', flexDirection: 'column', gap: 26}}>
          <Bar label="Raw tools" value={129358} max={129358} color={c.rose} frame={f} delay={36} />
          <Bar label="With RACC" value={1169} max={129358} color={c.emerald} frame={f} delay={70} />
        </div>
        <div style={{marginTop: 44}}>
          <Lede width={900}>
            Seven byte-honest scenarios against this repo. Same tokenizer on both sides, so the savings number is fair.
          </Lede>
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

const PipeNode: React.FC<{label: string; sub: string; accent: string; frame: number; delay: number}> = ({
  label,
  sub,
  accent,
  frame,
  delay,
}) => {
  const r = rise(frame, delay);
  return (
    <div
      style={{
        opacity: clamp(r),
        transform: `translateY(${(1 - r) * 30}px)`,
        width: 252,
        background: c.surfaceHi,
        border: `1px solid ${c.line}`,
        borderTop: `3px solid ${accent}`,
        borderRadius: 14,
        padding: '30px 26px',
      }}
    >
      <div style={{fontFamily: sans, fontSize: 34, fontWeight: 700, color: c.ink}}>{label}</div>
      <div style={{fontFamily: mono, fontSize: 19, color: c.muted, marginTop: 12, letterSpacing: 1}}>{sub}</div>
    </div>
  );
};

const Connector: React.FC<{frame: number; delay: number; accent: string}> = ({frame, delay, accent}) => {
  const g = ease(frame, delay, delay + 24);
  return (
    <div style={{width: 70, height: 2, background: c.line, position: 'relative'}}>
      <div style={{position: 'absolute', inset: 0, width: `${g * 100}%`, background: accent}} />
    </div>
  );
};

const ArchitectureScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 2);
  return (
    <SceneFrame scene={2}>
      <AbsoluteFill style={{justifyContent: 'center', padding: '0 96px'}}>
        <div style={{opacity: ease(f, 0, 14)}}>
          <Eyebrow color={c.sky}>Runtime path</Eyebrow>
          <div style={{marginTop: 22}}>
            <Display size={84}>Omitted bytes stay addressable.</Display>
          </div>
        </div>
        <div style={{marginTop: 76, display: 'flex', alignItems: 'center'}}>
          <PipeNode label="Agent" sub="read · find · shell" accent={c.sky} frame={f} delay={20} />
          <Connector frame={f} delay={40} accent={c.sky} />
          <PipeNode label="TokenZero" sub="render + store" accent={c.emerald} frame={f} delay={56} />
          <Connector frame={f} delay={76} accent={c.emerald} />
          <PipeNode label="Capsule" sub="small visible answer" accent={c.ink} frame={f} delay={92} />
          <Connector frame={f} delay={112} accent={c.gold} />
          <PipeNode label="tz:// ref" sub="exact recovery" accent={c.gold} frame={f} delay={128} />
        </div>
        <div style={{marginTop: 76, opacity: ease(f, 150, 180)}}>
          <Terminal
            width={1040}
            lines={[
              {t: '$ tokenzero read crates/tokenzero-mcp/src/lib.rs --json'}, 
              {t: '  visible: 150 tokens', c: c.emerald},
              {t: '  ref: tz://blob/...  (exact bytes, one expand away)', c: c.gold},
            ]}
          />
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

const E2EScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 3);
  const steps = [
    ['01', 'Tool call', 'Agent asks for a large file or a repo-wide grep.', c.sky],
    ['02', 'Capsule', 'TokenZero returns compact context plus refs.', c.emerald],
    ['03', 'Decision', 'Agent keeps working without dragging raw output.', c.ink],
    ['04', 'Recovery', 'expand returns byte-exact hidden bytes on demand.', c.gold],
  ] as const;
  return (
    <SceneFrame scene={3}>
      <AbsoluteFill style={{justifyContent: 'center', padding: '0 96px'}}>
        <div style={{opacity: ease(f, 0, 14)}}>
          <Eyebrow>End to end</Eyebrow>
          <div style={{marginTop: 22}}>
            <Display size={84}>One loop, charged honestly.</Display>
          </div>
        </div>
        <div style={{marginTop: 64, display: 'flex', gap: 24}}>
          {steps.map(([n, title, body, accent], i) => {
            const r = rise(f, 28 + i * 18);
            return (
              <div
                key={n}
                style={{
                  flex: 1,
                  opacity: clamp(r),
                  transform: `translateY(${(1 - r) * 44}px)`,
                  background: c.surface,
                  border: `1px solid ${c.line}`,
                  borderRadius: 18,
                  padding: 34,
                  minHeight: 320,
                }}
              >
                <div style={{fontFamily: mono, fontSize: 28, fontWeight: 700, color: accent, letterSpacing: 2}}>
                  {n}
                </div>
                <div style={{width: 38, height: 2, background: accent, margin: '22px 0'}} />
                <div style={{fontFamily: sans, fontSize: 36, fontWeight: 700, color: c.ink}}>{title}</div>
                <div style={{fontFamily: sans, fontSize: 25, lineHeight: 1.42, color: c.muted, marginTop: 18}}>
                  {body}
                </div>
              </div>
            );
          })}
        </div>
        <div style={{marginTop: 40, opacity: ease(f, 150, 180)}}>
          <Lede width={1000}>
            Savings count only after recovered bytes are charged back. A saving you cannot recover does not count.
          </Lede>
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

const Stat: React.FC<{value: string; label: string; accent: string; frame: number; delay: number}> = ({
  value,
  label,
  accent,
  frame,
  delay,
}) => {
  const r = rise(frame, delay);
  return (
    <div
      style={{
        flex: 1,
        opacity: clamp(r),
        transform: `translateY(${(1 - r) * 40}px)`,
        background: c.surface,
        border: `1px solid ${c.line}`,
        borderRadius: 18,
        padding: '42px 38px',
      }}
    >
      <div style={{fontFamily: sans, fontSize: 88, fontWeight: 800, letterSpacing: -3, color: accent}}>
        {value}
      </div>
      <div style={{fontFamily: mono, fontSize: 22, letterSpacing: 2, color: c.muted, marginTop: 16}}>
        {label}
      </div>
    </div>
  );
};

const MetricsScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 4);
  return (
    <SceneFrame scene={4}>
      <AbsoluteFill style={{justifyContent: 'center', padding: '0 96px'}}>
        <div style={{opacity: ease(f, 0, 14)}}>
          <Eyebrow>Live agent demo</Eyebrow>
          <div style={{marginTop: 22}}>
            <Display size={84}>Real Copilot CLI runs.</Display>
          </div>
        </div>
        <div style={{marginTop: 66, display: 'flex', gap: 24}}>
          <Stat value="92.7%" label="LESS TOOL-OUTPUT" accent={c.emerald} frame={f} delay={24} />
          <Stat value="1.68×" label="FASTER WALL-CLOCK" accent={c.sky} frame={f} delay={42} />
          <Stat value="2×" label="FEWER TOOL CALLS" accent={c.gold} frame={f} delay={60} />
        </div>
        <div style={{marginTop: 44, opacity: ease(f, 110, 150)}}>
          <Terminal
            width={1000}
            lines={[
              {t: '  baseline   51,823 tool-output tokens', c: c.rose},
              {t: '  tokenzero   3,786 tool-output tokens', c: c.emerald},
              {t: '  same task · same tokenizer · refs recover exact bytes', c: c.muted},
            ]}
          />
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

const ProofScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 5);
  const scan = interpolate(f, [40, 140], [-520, 520], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.inOut(Easing.cubic),
  });
  const stamp = rise(f, 150);
  return (
    <SceneFrame scene={5}>
      <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
        <div style={{textAlign: 'center', opacity: ease(f, 0, 16)}}>
          <Eyebrow color={c.gold}>Byte-exact proof</Eyebrow>
        </div>
        <div style={{marginTop: 28, opacity: ease(f, 8, 24)}}>
          <Display size={80}>Recover, then verify.</Display>
        </div>
        <div style={{marginTop: 56, position: 'relative', overflow: 'hidden', borderRadius: 16}}>
          <Terminal
            width={1020}
            lines={[
              {t: '$ tokenzero expand tz://blob/...'},
              {t: '  diff recovered bytes against original file', c: c.muted},
              {t: '  byte-exact recovery: PASS', c: c.emerald},
            ]}
          />
          <div
            style={{
              position: 'absolute',
              top: 0,
              left: scan,
              width: 160,
              height: '100%',
              background: `linear-gradient(90deg, transparent, ${c.emerald}55, transparent)`,
              filter: 'blur(4px)',
            }}
          />
        </div>
        <div
          style={{
            marginTop: 46,
            opacity: clamp(stamp),
            transform: `scale(${0.9 + stamp * 0.1})`,
            fontFamily: mono,
            fontSize: 24,
            letterSpacing: 3,
            color: c.emerald,
            border: `1px solid ${c.emerald}`,
            borderRadius: 10,
            padding: '14px 26px',
          }}
        >
          A SAVING YOU CANNOT RECOVER IS NOT A SAVING
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

const CmdBlock: React.FC<{os: string; lines: string[]; accent: string; frame: number; delay: number}> = ({
  os,
  lines,
  accent,
  frame,
  delay,
}) => {
  const r = rise(frame, delay);
  return (
    <div
      style={{
        opacity: clamp(r),
        transform: `translateY(${(1 - r) * 34}px)`,
        width: 880,
        background: c.surface,
        border: `1px solid ${c.line}`,
        borderLeft: `3px solid ${accent}`,
        borderRadius: 14,
        padding: '26px 30px',
      }}
    >
      <div style={{fontFamily: mono, fontSize: 20, letterSpacing: 3, color: accent, marginBottom: 18}}>
        {os}
      </div>
      {lines.map((l) => (
        <div key={l} style={{fontFamily: mono, fontSize: 25, lineHeight: 1.7, color: c.ink}}>
          <span style={{color: c.faint}}>$ </span>
          {l}
        </div>
      ))}
    </div>
  );
};

const ClosingScene: React.FC = () => {
  const f = localFrame(useCurrentFrame(), 6);
  return (
    <SceneFrame scene={6}>
      <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
        <div style={{textAlign: 'center', opacity: ease(f, 0, 16)}}>
          <Eyebrow>Try it</Eyebrow>
          <div style={{marginTop: 24}}>
            <Display size={78}>Run it on any OS.</Display>
          </div>
        </div>
        <div style={{marginTop: 52, display: 'flex', gap: 28}}>
          <CmdBlock
            os="MACOS / LINUX"
            accent={c.emerald}
            frame={f}
            delay={20}
            lines={[
              'pwsh -File ./demo/run_demo.ps1 -OpenViz',
              'pwsh -File ./demo/run_agent_demo.ps1 -Replicates 3',
            ]}
          />
          <CmdBlock
            os="WINDOWS"
            accent={c.sky}
            frame={f}
            delay={38}
            lines={[
              'pwsh -File .\\demo\\run_demo.ps1 -OpenViz',
              'pwsh -File .\\demo\\run_agent_demo.ps1 -Replicates 3',
            ]}
          />
        </div>
        <div style={{marginTop: 50, fontFamily: sans, fontSize: 27, color: c.muted, opacity: ease(f, 70, 100)}}>
          Compress aggressively. Recover exactly. Measure honestly.
        </div>
      </AbsoluteFill>
    </SceneFrame>
  );
};

export const RaccDemo: React.FC = () => {
  const {width, height} = useVideoConfig();
  return (
    <AbsoluteFill style={{width, height, overflow: 'hidden', background: c.bg}}>
      <TitleScene />
      <ProblemScene />
      <ArchitectureScene />
      <E2EScene />
      <MetricsScene />
      <ProofScene />
      <ClosingScene />
    </AbsoluteFill>
  );
};
