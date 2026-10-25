import {build} from 'esbuild';
for (const name of ['jquery','react','vue','preact','angular']) {
  await build({entryPoints:[`sources/${name}.js`],bundle:true,minify:true,format:'iife',target:'es2022',define:{'process.env.NODE_ENV':'"production"'},outfile:`${name}.js`,legalComments:'eof'});
}
