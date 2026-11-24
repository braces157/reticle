import './styles/main.css';
import {
  ArrowUpRight,
  BatteryFull,
  Check,
  ChevronLeft,
  ChevronRight,
  ClipboardCheck,
  Copy,
  Cpu,
  createIcons,
  Crop,
  Download,
  Grid3x3,
  Lock,
  Menu,
  Pin,
  Play,
  ShieldCheck,
  Sun,
  Terminal,
  Type,
  Volume2,
  Wifi,
  WifiOff,
  X,
} from 'lucide';
import { initArchPipeline } from './components/archPipeline';
import { initBento } from './components/bento';
import { initChrome } from './components/chrome';
import { initCommandPalette } from './components/commandPalette';
import { initHeroScan } from './components/heroScan';
import { initLiveSnip } from './components/liveSnip';
import { initOscilloscope } from './components/oscilloscope';
import { initPlayground } from './components/playground';
import { initSplitLab } from './components/splitLab';
import { initChoreography } from './motion/choreography';
import { initOpticalShader } from './motion/opticalShader';

createIcons({
  icons: {
    ArrowUpRight,
    BatteryFull,
    Check,
    ChevronLeft,
    ChevronRight,
    ClipboardCheck,
    Copy,
    Cpu,
    Crop,
    Download,
    Grid3x3,
    Lock,
    Menu,
    Pin,
    Play,
    ShieldCheck,
    Sun,
    Terminal,
    Type,
    Volume2,
    Wifi,
    WifiOff,
    X,
  },
});

initChrome();
initChoreography();
initOpticalShader();
initHeroScan();
initPlayground();
initSplitLab();
initBento();
initOscilloscope();
initArchPipeline();
const liveSnip = initLiveSnip();
initCommandPalette(liveSnip);
