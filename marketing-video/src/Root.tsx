import { Composition } from "remotion";
import { TwitterTeaser } from "./TwitterTeaser";
import { VerifiedPricing } from "./VerifiedPricing";

export const RemotionRoot: React.FC = () => {
  return (
    <>
      <Composition
        id="VerifiedPricing"
        component={VerifiedPricing}
        durationInFrames={660}
        fps={30}
        width={1920}
        height={1080}
      />
      <Composition
        id="TwitterTeaser"
        component={TwitterTeaser}
        durationInFrames={450}
        fps={30}
        width={1920}
        height={1080}
      />
    </>
  );
};
