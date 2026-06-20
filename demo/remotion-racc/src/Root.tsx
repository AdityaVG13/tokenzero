import React from 'react';
import {Composition} from 'remotion';
import {RaccDemo} from './RaccDemo';

export const Root: React.FC = () => (
  <Composition
    id="RaccDemo"
    component={RaccDemo}
    durationInFrames={1440}
    fps={30}
    width={1920}
    height={1080}
  />
);
