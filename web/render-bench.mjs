// Synthetic Canvas raster stress check; serve web/ before running.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const browser = await chromium.launch({executablePath:process.env.CHROMIUM_PATH || "/usr/bin/chromium",headless:true,args:["--no-sandbox"]});
try {
  const page = await browser.newPage();
  await page.goto(process.env.NES_PREVIEW_URL || "http://127.0.0.1:8765/");
  console.log(JSON.stringify(await page.evaluate(async () => {
    const {CanvasDepthRenderer}=await import("./depth-mesh.js");
    const {CanvasCardShadowRenderer}=await import("./card-shadow.js");
    const pixels=new Uint8Array(256*240*4), mask=new Uint8Array(pixels.length);
    for(let y=0;y<240;y++)for(let x=0;x<256;x++){
      if(y>210||(x%32<24&&y%32<24))pixels.set([160+x%80,90+y%90,40,255],(y*256+x)*4);
      if(x>110&&x<130&&y>100&&y<132)mask.set([220,60,40,255],(y*256+x)*4);
    }
    const source=document.createElement("canvas");source.width=256;source.height=240;
    source.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(pixels),256,240),0,0);
    const results=[];
    for(const [level,width] of [["high",1536],["balanced",768],["performance",512]]) {
      const front=document.createElement("canvas");
      const renderer=new CanvasDepthRenderer(document.createElement("canvas"),256,240,front);
      renderer.resize(768,720,width/768);
      const shadow=new CanvasCardShadowRenderer(document.createElement("canvas"));shadow.resize(768,720,width/768);
      const times=[];
      for(let n=0;n<100;n++){
        pixels[(220*256+n%256)*4]^=1;
        const start=performance.now();
        renderer.paint(pixels,4/19.2,null,undefined,{mode:"contact",softness:.36,overlapFraction:1/3,receiverRGBA:mask});
        shadow.paint(source,pixels,{frontDepth:0,backDepth:4/19.2,receiverDepth:85/19.2});
        // Force pending software/GPU raster to finish for a comparable sample.
        renderer.context.getImageData(0,0,1,1);
        shadow.context.getImageData(0,0,1,1);
        if(n>=10)times.push(performance.now()-start);
      }
      times.sort((a,b)=>a-b);
      results.push({level,effectWidth:renderer.canvas.width,sourceWidth:front.width,medianMs:times[times.length>>1],p95Ms:times[Math.floor(times.length*.95)]});
    }
    return results;
  }),null,2));
} finally {await browser.close();}
