// Original synthetic NROM scene; no commercial ROM or user data is needed.
export function fixtureRom({ backgroundIndex = 0x21 } = {}) {
  const prg = Buffer.alloc(16384, 0xea), chr = Buffer.alloc(8192);
  const code = [], labels = {}, fixups = [];
  const emit = (...bytes) => code.push(...bytes);
  const label = name => labels[name] = 0x8000 + code.length;
  const address = (opcode, name) => { emit(opcode,0,0); fixups.push([code.length-2,name,false]); };
  const branch = (opcode, name) => { emit(opcode,0); fixups.push([code.length-1,name,true]); };
  emit(0x78,0xd8,0xa2,0xff,0x9a,0xe8,0x8e,0,0x20,0x8e,1,0x20);
  label("wait"); emit(0x2c,2,0x20); branch(0x10,"wait");
  emit(0xa9,0x3f,0x8d,6,0x20,0xa9,0,0x8d,6,0x20,0xa2,0);
  label("paletteLoop"); address(0xbd,"palette"); emit(0x8d,7,0x20,0xe8,0xe0,32); branch(0xd0,"paletteLoop");
  emit(0xa9,0x20,0x8d,6,0x20,0xa9,0,0x8d,6,0x20,0xa2,0);
  label("tilesLoop");
  for (const name of ["tiles0","tiles1","tiles2","tiles3"]) { address(0xbd,name); emit(0x8d,7,0x20); }
  // Four interleaved pages are fine for a synthetic scene; all bytes are valid tiles.
  emit(0xe8); branch(0xd0,"tilesLoop");
  emit(0xa9,0,0x8d,5,0x20,0x8d,5,0x20,0xa9,0x0a,0x8d,1,0x20);
  label("forever"); address(0x4c,"forever");
  label("palette"); emit(...Array.from({length:32},(_,i)=>[backgroundIndex,0x19,0x29,0x30][i%4]));
  for (let n=0;n<4;n++) { label(`tiles${n}`); emit(...Array.from({length:256},(_,i)=> (i+n*256)>832 ? 2 : (i%32<4 || i%32>27) ? 1 : 0)); }
  for (const [offset,name,relative] of fixups) {
    if (relative) code[offset]=(labels[name]-(0x8000+offset+1))&255;
    else {code[offset]=labels[name]&255;code[offset+1]=labels[name]>>8;}
  }
  prg.set(code); for (const offset of [0x3ffa,0x3ffc,0x3ffe]) {prg[offset]=0;prg[offset+1]=0x80;}
  for(let y=0;y<8;y++){chr[16+y]=0xff;chr[24+y]=0;chr[32+y]=y===0?0xff:0x81;chr[40+y]=0xff;}
  return Buffer.concat([Buffer.from([0x4e,0x45,0x53,0x1a,1,1,0,0,0,0,0,0,0,0,0,0]),prg,chr]);
}
