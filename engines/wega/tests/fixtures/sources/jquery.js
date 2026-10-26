import $ from 'jquery'; $('#mount').append('<button id=action>0</button>');$('#action').on('click',function(){$(this).text(Number($(this).text())+1)});
globalThis.jQuery=$;globalThis.$=$;
