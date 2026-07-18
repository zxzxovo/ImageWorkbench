import { createEffect, createSignal } from "solid-js";
import { Brush, Eraser, FileUp, FlipHorizontal2, Hand, Minus, Plus, RotateCcw } from "lucide-solid";
import type { TranslationKey } from "../lib/i18n";
import { IconButton } from "./common";

interface MaskCanvasProps {
  t: (key: TranslationKey) => string;
  sourceUrl?: string;
  sourceWidth: number;
  sourceHeight: number;
  initialMaskDataUrl?: string;
  onChange: (dataUrl: string) => void;
}

export default function MaskCanvas(props: MaskCanvasProps) {
  let canvas!: HTMLCanvasElement;
  let fileInput!: HTMLInputElement;
  const [tool, setTool] = createSignal<"brush" | "eraser" | "pan">("brush");
  const [brushSize, setBrushSize] = createSignal(34);
  const [zoom, setZoom] = createSignal(1);
  const [pan, setPan] = createSignal({ x: 0, y: 0 });
  let drawing = false;
  let panning = false;
  let lastPoint: { x: number; y: number } | null = null;
  let panStart: { x: number; y: number; originX: number; originY: number } | null = null;

  const context = () => canvas.getContext("2d");

  const emit = () => props.onChange(canvas.toDataURL("image/png"));

  const clear = () => {
    const ctx = context();
    if (!ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    props.onChange("");
  };

  const invert = () => {
    const ctx = context();
    if (!ctx) return;
    const image = ctx.getImageData(0, 0, canvas.width, canvas.height);
    for (let index = 0; index < image.data.length; index += 4) {
      const alpha = image.data[index + 3];
      image.data[index] = 17;
      image.data[index + 1] = 19;
      image.data[index + 2] = 22;
      image.data[index + 3] = 255 - alpha;
    }
    ctx.putImageData(image, 0, 0);
    emit();
  };

  const canvasPoint = (event: PointerEvent) => {
    const rect = canvas.getBoundingClientRect();
    return {
      x: (event.clientX - rect.left) * (canvas.width / rect.width),
      y: (event.clientY - rect.top) * (canvas.height / rect.height),
    };
  };

  const startDrawing = (event: PointerEvent) => {
    if (tool() === "pan") {
      panning = true;
      panStart = { x: event.clientX, y: event.clientY, originX: pan().x, originY: pan().y };
      canvas.setPointerCapture(event.pointerId);
      return;
    }
    drawing = true;
    lastPoint = canvasPoint(event);
    canvas.setPointerCapture(event.pointerId);
  };

  const draw = (event: PointerEvent) => {
    if (panning && panStart) {
      setPan({
        x: panStart.originX + event.clientX - panStart.x,
        y: panStart.originY + event.clientY - panStart.y,
      });
      return;
    }
    if (!drawing || !lastPoint) return;
    const next = canvasPoint(event);
    const ctx = context();
    if (!ctx) return;
    ctx.save();
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.lineWidth = brushSize();
    ctx.globalCompositeOperation = tool() === "eraser" ? "destination-out" : "source-over";
    ctx.strokeStyle = "rgba(16, 18, 21, 0.78)";
    ctx.beginPath();
    ctx.moveTo(lastPoint.x, lastPoint.y);
    ctx.lineTo(next.x, next.y);
    ctx.stroke();
    ctx.restore();
    lastPoint = next;
  };

  const stopDrawing = () => {
    if (panning) {
      panning = false;
      panStart = null;
      return;
    }
    if (!drawing) return;
    drawing = false;
    lastPoint = null;
    emit();
  };

  const changeZoom = (next: number) => setZoom(Math.min(3, Math.max(0.5, Math.round(next * 10) / 10)));
  const resetView = () => { setZoom(1); setPan({ x: 0, y: 0 }); };

  const importMask = (file: File | undefined) => {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const image = new Image();
      image.onload = () => {
        const ctx = context();
        if (!ctx) return;
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
        emit();
      };
      image.src = String(reader.result);
    };
    reader.readAsDataURL(file);
  };

  let loadedMaskKey = "";
  createEffect(() => {
    const mask = props.initialMaskDataUrl?.trim() ?? "";
    const key = `${props.sourceUrl ?? ""}|${props.sourceWidth}x${props.sourceHeight}|${mask}`;
    if (key === loadedMaskKey) return;
    loadedMaskKey = key;
    if (!mask) props.onChange("");
    const ctx = context();
    if (!ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (!mask) {
      return;
    }
    const image = new Image();
    image.onload = () => {
      const next = context();
      if (!next || key !== loadedMaskKey) return;
      next.clearRect(0, 0, canvas.width, canvas.height);
      next.drawImage(image, 0, 0, canvas.width, canvas.height);
    };
    image.src = mask;
  });

  return (
    <section class="mask-editor">
      <header class="mask-toolbar">
        <div class="segmented icon-segmented" aria-label={props.t("maskEditor")}>
          <IconButton label={props.t("brush")} active={tool() === "brush"} onClick={() => setTool("brush")}><Brush size={16} /></IconButton>
          <IconButton label={props.t("eraser")} active={tool() === "eraser"} onClick={() => setTool("eraser")}><Eraser size={16} /></IconButton>
          <IconButton label={props.t("panTool")} active={tool() === "pan"} onClick={() => setTool("pan")}><Hand size={16} /></IconButton>
        </div>
        <label class="mask-size-control">
          <span>{props.t("brushSize")}</span>
          <input type="range" min="8" max="96" value={brushSize()} onInput={(event) => setBrushSize(Number(event.currentTarget.value))} />
          <output>{brushSize()}</output>
        </label>
        <span class="toolbar-spacer" />
        <IconButton label={props.t("zoomOut")} onClick={() => changeZoom(zoom() - 0.1)}><Minus size={16} /></IconButton>
        <button class="mask-zoom-value" type="button" title={props.t("resetView")} onClick={resetView}>{Math.round(zoom() * 100)}%</button>
        <IconButton label={props.t("zoomIn")} onClick={() => changeZoom(zoom() + 0.1)}><Plus size={16} /></IconButton>
        <IconButton label={props.t("invert")} onClick={invert}><FlipHorizontal2 size={16} /></IconButton>
        <IconButton label={props.t("clear")} onClick={clear}><RotateCcw size={16} /></IconButton>
        <IconButton label={props.t("importMask")} onClick={() => fileInput.click()}><FileUp size={16} /></IconButton>
        <input
          ref={fileInput}
          class="visually-hidden"
          type="file"
          accept="image/png,image/jpeg,image/webp"
          onChange={(event) => importMask(event.currentTarget.files?.[0])}
        />
      </header>
      <div class="mask-stage" style={{ "aspect-ratio": `${props.sourceWidth} / ${props.sourceHeight}`, width: `min(100%, calc(520px * ${props.sourceWidth} / ${props.sourceHeight}))` }} onWheel={(event) => { event.preventDefault(); changeZoom(zoom() + (event.deltaY < 0 ? 0.1 : -0.1)); }}>
        <div class="mask-viewport" style={{ transform: `translate(${pan().x}px, ${pan().y}px) scale(${zoom()})`, "background-image": props.sourceUrl ? `url(${props.sourceUrl})` : undefined }}>
          <canvas
            ref={canvas}
            width={props.sourceWidth}
            height={props.sourceHeight}
            class={tool() === "pan" ? "is-panning" : ""}
            onPointerDown={startDrawing}
            onPointerMove={draw}
            onPointerUp={stopDrawing}
            onPointerCancel={stopDrawing}
            onPointerLeave={stopDrawing}
          />
        </div>
      </div>
    </section>
  );
}
